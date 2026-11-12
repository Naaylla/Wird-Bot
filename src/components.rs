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

    let data = Data::from_context(ctx).await?;
    let user_id = component.user.id.get();
    let inserted = db::mark_done(data.db.clone(), user_id, day, verse_key).await?;

    let message = if inserted {
        let stats = db::stats(data.db.clone(), user_id, Utc::now().date_naive()).await?;
        format!(
            "MashaAllah! You memorized {verse_key} today. Current streak: {} day(s). Come back tomorrow for the next ayah.",
            stats.streak
        )
    } else {
        "You already memorized today's ayah. One ayah a day — come back tomorrow, in shaa Allah."
            .to_owned()
    };

    reply_ephemeral(ctx, component, &message).await
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
