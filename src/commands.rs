use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    CommandInteraction, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use tracing::warn;

use crate::data::Data;
use crate::db;
use crate::quran;
use crate::ui;

pub async fn wird(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
        )
        .await?;

    let data = Data::from_context(ctx).await?;
    let day = Utc::now().date_naive();
    let day_string = day.format("%Y-%m-%d").to_string();
    let verse_key = quran::daily_verse_key(day);
    let user_id = command.user.id.get();

    let ayah = match data.api.ayah(&verse_key).await {
        Ok(ayah) => ayah,
        Err(err) => {
            warn!("could not fetch {verse_key}: {err:#}");
            command
                .edit_response(
                    ctx.http.clone(),
                    EditInteractionResponse::new()
                        .content("The Qur'an API did not answer. Please try again in a moment."),
                )
                .await?;
            return Ok(());
        }
    };

    let done = db::is_done(data.db.clone(), user_id, &day_string).await?;
    let (surah, _) = quran::verse_parts(&ayah.verse_key);
    let chapter = data.chapters.get(&surah);

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new()
                .embed(ui::daily_embed(day, &ayah, chapter, done))
                .components(vec![ui::daily_buttons(day, &ayah.verse_key, done)]),
        )
        .await?;

    Ok(())
}

pub async fn leaderboard(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
        )
        .await?;

    let data = Data::from_context(ctx).await?;
    let today = Utc::now().date_naive();
    let entries = db::leaderboard(data.db.clone(), today, 15).await?;

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new().embed(ui::leaderboard_embed(&entries, today)),
        )
        .await?;

    Ok(())
}

pub async fn progress(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

    let data = Data::from_context(ctx).await?;
    let user_id = command.user.id.get();
    let stats = db::stats(data.db.clone(), user_id, Utc::now().date_naive()).await?;

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new().embed(ui::progress_embed(
                stats.total,
                stats.streak,
                stats.done_today,
            )),
        )
        .await?;

    Ok(())
}
