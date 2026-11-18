use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, Context, CreateAutocompleteResponse,
    CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
};
use tracing::warn;

use crate::chapters;
use crate::data::Data;
use crate::db;
use crate::quran;
use crate::ui;

pub async fn autocomplete(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let query = command
        .data
        .options
        .iter()
        .find_map(|option| match &option.value {
            CommandDataOptionValue::Autocomplete { value, .. } => Some(value.to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();

    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    let mut response = CreateAutocompleteResponse::new();
    let mut choice_count = 0;

    for (surah, chapter) in chapters.iter() {
        let label = format!("{surah}. {} ({})", chapter.name, chapter.arabic);
        let matches = query.is_empty()
            || label.to_lowercase().contains(&query)
            || surah.to_string().starts_with(&query);
        if matches {
            response = response.add_string_choice(label, surah.to_string());
            choice_count += 1;
            if choice_count == 25 {
                break;
            }
        }
    }

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Autocomplete(response),
        )
        .await?;

    Ok(())
}

pub async fn plan(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    let mode = option_string(command, "mode").unwrap_or_default();
    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    let target_surah = match mode {
        "quran" => None,
        "surah" => {
            let Some(surah) = option_string(command, "surah")
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|surah| chapters.contains_key(surah))
            else {
                return reply_ephemeral(
                    ctx,
                    command,
                    "Choose a surah from the autocomplete list to start a surah plan.",
                )
                .await;
            };
            Some(surah)
        }
        _ => {
            return reply_ephemeral(
                ctx,
                command,
                "Choose either the full Qur'an plan or a single-surah plan.",
            )
            .await;
        }
    };

    let plan = db::set_plan(
        data.db.clone(),
        guild_id,
        command.user.id.get(),
        target_surah,
    )
    .await?;
    let name = match target_surah {
        Some(surah) => chapters
            .get(&surah)
            .map(|chapter| chapter.name.as_str())
            .unwrap_or("the selected surah"),
        None => "the full Qur'an",
    };
    let message = match plan.next_verse {
        Some((surah, ayah)) => format!(
            "Your personal plan is set to memorize {name} in order. Your current ayah is {surah}:{ayah}. Use `/wird` to read it."
        ),
        None => format!(
            "You have completed {name}. Choose a plan again when you are ready to start another."
        ),
    };

    reply_ephemeral(ctx, command, &message).await
}

pub async fn wird(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    let user_id = command.user.id.get();
    let Some(plan) = db::get_plan(data.db.clone(), guild_id, user_id).await? else {
        return reply_ephemeral(
            ctx,
            command,
            "Choose a memorization plan with `/plan` before starting. You can memorize the full Qur'an or choose one surah.",
        )
        .await;
    };

    let day = Utc::now().date_naive();
    let day_string = day.format("%Y-%m-%d").to_string();
    let completed_today =
        db::verse_for_day(data.db.clone(), guild_id, user_id, &day_string).await?;
    let done = completed_today
        .as_deref()
        .is_some_and(|completed| is_current_plan_ayah(&chapters, plan, completed));
    let blocked_today = completed_today.is_some() && !done;
    let verse_key = match (done, completed_today.as_deref()) {
        (true, Some(verse_key)) => verse_key.to_owned(),
        _ => match plan.next_verse {
            Some((surah, ayah)) => format!("{surah}:{ayah}"),
            None => {
                let message = match plan.target_surah {
                    Some(surah) => format!(
                        "You have finished Surah {}. Use `/plan` to choose another plan.",
                        chapters
                            .get(&surah)
                            .map(|chapter| chapter.name.as_str())
                            .unwrap_or("you selected")
                    ),
                    None => {
                        "You have finished the whole Qur'an. Use `/plan` if you would like to begin again."
                            .to_owned()
                    }
                };
                return reply_ephemeral(ctx, command, &message).await;
            }
        },
    };

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

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

    let (surah, _) = quran::verse_parts(&ayah.verse_key);
    let chapter = chapters.get(&surah);

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new()
                .embed(ui::daily_embed(day, &ayah, chapter, done, blocked_today))
                .components(vec![ui::daily_buttons(
                    day,
                    &ayah.verse_key,
                    done,
                    blocked_today,
                )]),
        )
        .await?;

    Ok(())
}

pub async fn leaderboard(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
        )
        .await?;

    let data = Data::from_context(ctx).await?;
    let today = Utc::now().date_naive();
    let entries = db::leaderboard(data.db.clone(), guild_id, today, 15).await?;

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new().embed(ui::leaderboard_embed(&entries, today)),
        )
        .await?;

    Ok(())
}

pub async fn progress(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

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
    let stats = db::stats(data.db.clone(), guild_id, user_id, Utc::now().date_naive()).await?;

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

pub async fn reset(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(_guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(
                        "Reset your progress in this server?\n\nThis deletes every ayah you have memorized here and restarts your plan from the first ayah. This cannot be undone.",
                    )
                    .components(vec![ui::reset_prompt_buttons()])
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

fn guild_id(command: &CommandInteraction) -> Option<u64> {
    command.guild_id.map(|guild| guild.get())
}

fn option_string<'a>(command: &'a CommandInteraction, name: &str) -> Option<&'a str> {
    command
        .data
        .options
        .iter()
        .find(|option| option.name == name)
        .and_then(|option| match &option.value {
            CommandDataOptionValue::String(value) => Some(value.as_str()),
            _ => None,
        })
}

fn is_current_plan_ayah(
    chapters: &chapters::ChapterList,
    plan: db::Plan,
    completed_verse: &str,
) -> bool {
    let expected = match plan.next_verse {
        Some(current) => chapters::previous_verse(chapters, current, plan.target_surah),
        None => chapters::final_verse(chapters, plan.target_surah),
    };

    expected
        .map(|(surah, ayah)| format!("{surah}:{ayah}"))
        .as_deref()
        == Some(completed_verse)
}

async fn reply_ephemeral(ctx: &Context, command: &CommandInteraction, message: &str) -> Result<()> {
    command
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_previous_plan_completion_does_not_replace_the_new_plan_ayah() {
        let chapters: chapters::ChapterList = [(
            10,
            chapters::Chapter {
                name: "Yunus".to_owned(),
                arabic: "يونس".to_owned(),
                verses: 109,
            },
        )]
        .into_iter()
        .collect();

        let plan = db::Plan {
            target_surah: Some(10),
            next_verse: Some((10, 1)),
        };
        assert!(!is_current_plan_ayah(&chapters, plan, "31:11"));
        assert!(!is_current_plan_ayah(&chapters, plan, "10:1"));

        let next_ayah = db::Plan {
            target_surah: Some(10),
            next_verse: Some((10, 2)),
        };
        assert!(is_current_plan_ayah(&chapters, next_ayah, "10:1"));

        let completed_surah = db::Plan {
            target_surah: Some(10),
            next_verse: None,
        };
        assert!(is_current_plan_ayah(&chapters, completed_surah, "10:109"));
    }
}
