use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    ComponentInteraction, ComponentInteractionDataKind, Context, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};
use tracing::warn;

use crate::chapters;
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
        "surah_page" => {
            let page = parts.next().unwrap_or_default();
            surah_page(ctx, component, page).await
        }
        "surah_select" => surah_select(ctx, component).await,
        "daily_select" => {
            let target = parts.next().unwrap_or_default();
            daily_select(ctx, component, target).await
        }
        "tafsir_select" => tafsir_prompt(ctx, component).await,
        "help" => help(ctx, component).await,
        _ => Ok(()),
    }
}

async fn help(ctx: &Context, component: &ComponentInteraction) -> Result<()> {
    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .embed(ui::help_embed())
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
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
    let chapters = data.chapters.snapshot().await;
    let user_id = component.user.id.get();

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

    let completed_today = db::verses_for_day(data.db.clone(), guild_id, user_id, day).await?;
    if completed_today
        .iter()
        .any(|completed| completed == verse_key)
    {
        return reply_ephemeral(
            ctx,
            component,
            "You already memorized this ayah today. Run `/wird` to continue, in shaa Allah.",
        )
        .await;
    }
    if completed_today.len() as u16 >= plan.daily_target {
        return reply_ephemeral(
            ctx,
            component,
            &format!(
                "You have reached today's wird of {} ayah(s). Come back tomorrow, in shaa Allah.",
                plan.daily_target
            ),
        )
        .await;
    }

    let next = chapters::next_verse(&chapters, current, plan.target_surah);
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
            let completed = completed_today.len() as u16 + 1;
            let follow_up = if completed < plan.daily_target {
                "Tap the next ayah's button in today's block."
            } else {
                "Your next ayah will be available tomorrow."
            };
            format!(
                "MashaAllah! You memorized {verse_key} today. {completed}/{} ayahs done. Current streak: {} day(s). {follow_up}",
                plan.daily_target, stats.streak
            )
        }
        db::MarkResult::AlreadyDone => {
            "You already memorized this ayah today. Run `/wird` to continue.".to_owned()
        }
        db::MarkResult::DailyLimit => format!(
            "You have reached today's wird of {} ayah(s). Come back tomorrow, in shaa Allah.",
            plan.daily_target
        ),
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

async fn surah_page(ctx: &Context, component: &ComponentInteraction, page: &str) -> Result<()> {
    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;

    let (content, components) = if chapters.is_empty() {
        (
            "The surah list is still loading. Please try again in a minute.".to_owned(),
            Vec::new(),
        )
    } else {
        ui::surah_picker(page.parse().unwrap_or(0), &chapters)
    };

    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .components(components),
            ),
        )
        .await?;

    Ok(())
}

async fn surah_select(ctx: &Context, component: &ComponentInteraction) -> Result<()> {
    let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
        return Ok(());
    };
    let Some(surah) = values.first().and_then(|value| value.parse::<u16>().ok()) else {
        return reply_ephemeral(ctx, component, "Choose a surah from the list to continue.").await;
    };

    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    if !chapters.contains_key(&surah) {
        return reply_ephemeral(ctx, component, "That surah is not available right now.").await;
    };

    let (content, components) = ui::daily_picker(Some(surah));
    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .components(components),
            ),
        )
        .await?;

    Ok(())
}

async fn daily_select(ctx: &Context, component: &ComponentInteraction, target: &str) -> Result<()> {
    let Some(guild_id) = component.guild_id else {
        return reply_ephemeral(ctx, component, "Use this in a server.").await;
    };
    let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
        return Ok(());
    };
    let Some(daily_target) = values
        .first()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (1..=10).contains(value))
    else {
        return reply_ephemeral(ctx, component, "Choose between 1 and 10 ayahs per day.").await;
    };

    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    let target_surah = if target == "quran" {
        None
    } else {
        let Some(surah) = target
            .parse::<u16>()
            .ok()
            .filter(|surah| chapters.contains_key(surah))
        else {
            return reply_ephemeral(ctx, component, "That surah is not available right now.").await;
        };
        Some(surah)
    };
    let plan = db::set_plan(
        data.db.clone(),
        guild_id.get(),
        component.user.id.get(),
        target_surah,
        Some(daily_target),
    )
    .await?;
    let name = target_surah
        .and_then(|surah| chapters.get(&surah))
        .map(|chapter| chapter.name.as_str())
        .unwrap_or("the full Qur'an");
    let pace = if plan.daily_target == 1 {
        "1 ayah per day".to_owned()
    } else {
        format!("{} ayahs per day", plan.daily_target)
    };
    let message = match plan.next_verse {
        Some((surah, ayah)) => format!(
            "Your personal plan is set to memorize {name} in order, {pace}. Your current ayah is {surah}:{ayah}. Use `/wird` to see today's ayahs."
        ),
        None => format!(
            "You have completed {name}. Choose a plan again when you are ready to start another."
        ),
    };

    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(message)
                    .components(Vec::new()),
            ),
        )
        .await?;

    Ok(())
}

async fn tafsir_prompt(ctx: &Context, component: &ComponentInteraction) -> Result<()> {
    let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind else {
        return Ok(());
    };
    let Some(verse_key) = values.first() else {
        return reply_ephemeral(ctx, component, "Choose an ayah for tafsir.").await;
    };
    let Some((surah, ayah)) = verse_key
        .split_once(':')
        .and_then(|(surah, ayah)| Some((surah.parse::<u16>().ok()?, ayah.parse::<u16>().ok()?)))
    else {
        return reply_ephemeral(ctx, component, "That ayah is not available.").await;
    };
    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    if !chapters
        .get(&surah)
        .is_some_and(|chapter| (1..=chapter.verses).contains(&ayah))
    {
        return reply_ephemeral(ctx, component, "That ayah is not available.").await;
    }

    component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!("Choose a tafsir for **{verse_key}**."))
                    .components(vec![ui::tafsir_buttons(verse_key)])
                    .ephemeral(true),
            ),
        )
        .await?;

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
    let chapters = data.chapters.snapshot().await;
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
    let chapter = chapters.get(&surah);

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
