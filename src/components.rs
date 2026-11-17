use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    ComponentInteraction, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use tracing::warn;

use crate::data::Data;
use crate::db;
use crate::quran;
use crate::ui;

pub async fn handle(ctx: &Context, component: &ComponentInteraction) -> Result<()> {
    let mut parts = component.data.custom_id.split('|');

    if parts.next() != Some("wird") {
        return Ok(());
    }

    let action = parts.next().unwrap_or_default();

    match action {
        "memorized" => {
            let day = parts.next().unwrap_or_default();
            let verse_key = parts.next().unwrap_or_default();
            memorized(ctx, component, day, verse_key).await
        }
        "tafsir_en" => {
            let verse_key = parts.next().unwrap_or_default();
            tafsir(
                ctx,
                component,
                verse_key,
                quran::TAFSIR_EN_ID,
                "Ibn Kathir (English)",
            )
            .await
        }
        "tafsir_ar" => {
            let verse_key = parts.next().unwrap_or_default();
            tafsir(
                ctx,
                component,
                verse_key,
                quran::TAFSIR_AR_ID,
                "Tafsir al-Muyassar (Arabic)",
            )
            .await
        }
        "reset" => {
            let step = parts.next().unwrap_or_default();
            reset(ctx, component, step).await
        }
        _ => Ok(()),
    }
}

async fn memorized(
    ctx: &Context,
    component: &ComponentInteraction,
    day: &str,
    verse_key: &str,
) -> Result<()> {
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

    if day != today {
        return reply_ephemeral(
            ctx,
            component,
            "That ayah is from a previous day. Run /wird to get today's ayah.",
        )
        .await;
    }

    let Some(guild_id) = component.guild_id else {
        return reply_ephemeral(ctx, component, "Use this in a server.").await;
    };
    let guild_id = guild_id.get();

    let data = Data::from_context(ctx).await?;
    let user_id = component.user.id.get();
    if db::verse_for_day(data.db.clone(), guild_id, user_id, day)
        .await?
        .is_some()
    {
        return reply_ephemeral(
            ctx,
            component,
            "You already memorized today's ayah. One ayah a day — come back tomorrow, in shaa Allah.",
        )
        .await;
    }

    let Some(plan) = db::get_plan(data.db.clone(), guild_id, user_id).await? else {
        return reply_ephemeral(
            ctx,
            component,
            "Choose a memorization plan with `/plan` first.",
        )
        .await;
    };
    let Some(current) = plan.next_verse else {
        return reply_ephemeral(
            ctx,
            component,
            "This plan is complete. Use `/plan` to continue.",
        )
        .await;
    };
    let current_key = format!("{}:{}", current.0, current.1);
    if current_key != verse_key {
        return reply_ephemeral(
            ctx,
            component,
            "That is not your current ayah. Run `/wird` to get your current one.",
        )
        .await;
    }

    let next = quran::next_verse(current, plan.target_surah);
    let result = db::mark_done(
        data.db.clone(),
        guild_id,
        user_id,
        day,
        verse_key,
        plan,
        next,
    )
    .await?;

    let message = match result {
        db::MarkResult::Inserted => {
            let stats =
                db::stats(data.db.clone(), guild_id, user_id, Utc::now().date_naive()).await?;
            format!(
                "MashaAllah! You memorized {verse_key} today. Current streak: {} day(s). Your next ayah will be available tomorrow.",
                stats.streak
            )
        }
        db::MarkResult::AlreadyDone => {
            "You already memorized today's ayah. One ayah a day — come back tomorrow, in shaa Allah."
                .to_owned()
        }
        db::MarkResult::Stale => {
            "Your plan has changed. Run `/wird` to get your current ayah.".to_owned()
        }
    };

    reply_ephemeral(ctx, component, &message).await?;

    if result == db::MarkResult::Inserted {
        component
            .channel_id
            .say(&ctx.http, ui::memorization_announcement(user_id, verse_key))
            .await?;
    }

    Ok(())
}

async fn reset(ctx: &Context, component: &ComponentInteraction, step: &str) -> Result<()> {
    let Some(guild_id) = component.guild_id else {
        return reply_ephemeral(ctx, component, "Use this in a server.").await;
    };
    let guild_id = guild_id.get();

    let message = match step {
        "confirm_prompt" => CreateInteractionResponseMessage::new()
            .content(
                "Are you absolutely sure? Your streak and every ayah you memorized in this server will be deleted, and your plan will restart from the first ayah. This cannot be undone.",
            )
            .components(vec![ui::reset_confirm_buttons()]),
        "cancel" => CreateInteractionResponseMessage::new()
            .content("Reset cancelled. Nothing was changed.")
            .components(Vec::new()),
        "confirm" => {
            let data = Data::from_context(ctx).await?;
            db::reset(
                data.db.clone(),
                guild_id,
                component.user.id.get(),
            )
            .await?;
            CreateInteractionResponseMessage::new()
                .content(
                    "Your progress has been reset. Run `/wird` to start again from the first ayah of your plan, or `/plan` to choose a different plan.",
                )
                .components(vec![ui::reset_cancel_button()])
        }
        "dismiss" => CreateInteractionResponseMessage::new()
            .content(
                "Your progress has been reset. Run `/wird` to start again from the first ayah of your plan, or `/plan` to choose a different plan.",
            )
            .components(Vec::new()),
        _ => return Ok(()),
    };

    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::UpdateMessage(message),
        )
        .await?;

    Ok(())
}

async fn tafsir(
    ctx: &Context,
    component: &ComponentInteraction,
    verse_key: &str,
    tafsir_id: u32,
    source: &str,
) -> Result<()> {
    component.defer_ephemeral(ctx.http.clone()).await?;

    let data = Data::from_context(ctx).await?;
    let text = match data.api.tafsir(tafsir_id, verse_key).await {
        Ok(text) => text,
        Err(err) => {
            warn!("could not fetch tafsir {tafsir_id} for {verse_key}: {err:#}");
            component
                .edit_response(
                    ctx.http.clone(),
                    EditInteractionResponse::new()
                        .content("The tafsir could not be loaded. Please try again in a moment."),
                )
                .await?;
            return Ok(());
        }
    };

    let (surah, _) = quran::verse_parts(verse_key);
    let chapter = data.chapters.get(&surah);

    component
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new()
                .embed(ui::tafsir_embed(verse_key, chapter, source, &text)),
        )
        .await?;

    Ok(())
}

async fn reply_ephemeral(
    ctx: &Context,
    component: &ComponentInteraction,
    message: &str,
) -> Result<()> {
    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(message)
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}
