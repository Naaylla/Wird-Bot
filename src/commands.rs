use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, Context, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};
use tracing::warn;

use crate::chapters;
use crate::data::Data;
use crate::db;
use crate::ui;

pub async fn plan(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    if guild_id(command).is_none() {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    }

    let mode = option_string(command, "mode").unwrap_or_default();
    let data = Data::from_context(ctx).await?;
    let chapters = data.chapters.snapshot().await;
    match mode {
        "quran" => {
            let (content, components) = ui::daily_picker(None);
            command
                .create_response(
                    ctx.http.clone(),
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .components(components)
                            .ephemeral(true),
                    ),
                )
                .await?;
        }
        "surah" => {
            if chapters.is_empty() {
                return reply_ephemeral(
                    ctx,
                    command,
                    "The surah list is still loading. Please try again in a minute.",
                )
                .await;
            }
            let (content, components) = ui::surah_picker(0, &chapters);
            command
                .create_response(
                    ctx.http.clone(),
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .components(components)
                            .ephemeral(true),
                    ),
                )
                .await?;
        }
        _ => {
            return reply_ephemeral(
                ctx,
                command,
                "Choose either the full Qur'an plan or a single-surah plan.",
            )
            .await;
        }
    }

    Ok(())
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
        db::verses_for_day(data.db.clone(), guild_id, user_id, &day_string).await?;
    let completed = completed_today.len() as u16;
    let target = plan.daily_target;

    let verse_keys = ayahs_to_show(&chapters, plan, &completed_today);
    if verse_keys.is_empty() {
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

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

    let mut ayahs = Vec::with_capacity(verse_keys.len());
    for verse_key in &verse_keys {
        match data.api.ayah(verse_key).await {
            Ok(ayah) => ayahs.push(ayah),
            Err(err) => {
                warn!("could not fetch {verse_key}: {err:#}");
                command
                    .edit_response(
                        ctx.http.clone(),
                        EditInteractionResponse::new().content(
                            "The Qur'an API did not answer. Please try again in a moment.",
                        ),
                    )
                    .await?;
                return Ok(());
            }
        }
    }

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new()
                .embeds(ui::daily_embeds(
                    &ayahs,
                    &chapters,
                    completed,
                    target,
                    plan.next_verse.is_none(),
                ))
                .components(ui::daily_buttons(day, &ayahs, &completed_today)),
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

pub async fn remind(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    let data = Data::from_context(ctx).await?;
    let user_id = command.user.id.get();

    if option_bool(command, "disable") == Some(true) {
        let removed = db::clear_reminder(data.db.clone(), guild_id, user_id).await?;
        let message = if removed {
            "Your daily reminder is off."
        } else {
            "You did not have a daily reminder set."
        };
        return reply_ephemeral(ctx, command, message).await;
    }

    let existing = db::get_reminder(data.db.clone(), guild_id, user_id).await?;

    let Some(raw_time) = option_string(command, "time") else {
        let message = match existing {
            Some(reminder) => format!(
                "Your daily reminder is set for **{}** ({}). Use `/remind disable:true` to turn it off.",
                reminder.time,
                format_offset(reminder.offset_minutes)
            ),
            None => "No daily reminder set. Use `/remind time:19:30 offset:+03:00` to be reminded when your wird is unfinished.".to_owned(),
        };
        return reply_ephemeral(ctx, command, &message).await;
    };

    let Some(time) = parse_time(raw_time) else {
        return reply_ephemeral(ctx, command, "Use a 24-hour time like `19:30`.").await;
    };
    let offset = match option_string(command, "offset") {
        Some(raw) => match parse_offset(raw) {
            Some(offset) => offset,
            None => {
                return reply_ephemeral(
                    ctx,
                    command,
                    "Use a UTC offset like `+03:00`, `+05:30` or `-04:00`.",
                )
                .await;
            }
        },
        None => existing
            .as_ref()
            .map(|reminder| reminder.offset_minutes)
            .unwrap_or(0),
    };

    db::set_reminder(data.db.clone(), guild_id, user_id, &time, offset).await?;

    let message = format!(
        "Reminder set for **{time}** ({}). I will DM you when that day's wird is unfinished.",
        format_offset(offset)
    );
    reply_ephemeral(ctx, command, &message).await
}

pub async fn help(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    command
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

fn option_bool(command: &CommandInteraction, name: &str) -> Option<bool> {
    command
        .data
        .options
        .iter()
        .find(|option| option.name == name)
        .and_then(|option| match &option.value {
            CommandDataOptionValue::Boolean(value) => Some(*value),
            _ => None,
        })
}

fn ayahs_to_show(
    chapters: &chapters::ChapterList,
    plan: db::Plan,
    completed_today: &[String],
) -> Vec<String> {
    if completed_today.len() as u16 >= plan.daily_target || plan.next_verse.is_none() {
        return completed_today.to_vec();
    }

    let mut current = plan.next_verse;
    let count = plan
        .daily_target
        .saturating_sub(completed_today.len() as u16);
    let mut verses = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let Some((surah, ayah)) = current else {
            break;
        };
        verses.push(format!("{surah}:{ayah}"));
        current = chapters::next_verse(chapters, (surah, ayah), plan.target_surah);
    }
    verses
}

fn parse_time(raw: &str) -> Option<String> {
    let (hour, minute) = raw.trim().split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }

    Some(format!("{hour:02}:{minute:02}"))
}

fn parse_offset(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    let sign = match raw.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (hour, minute) = raw[1..].split_once(':')?;
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    if hour > 14 || minute > 59 || (hour == 14 && minute > 0) {
        return None;
    }

    Some(sign * (hour * 60 + minute))
}

fn format_offset(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = offset_minutes.abs();
    format!("UTC{sign}{:02}:{:02}", offset / 60, offset % 60)
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

    fn fixture() -> chapters::ChapterList {
        [
            (
                1,
                chapters::Chapter {
                    name: "Al-Fatihah".to_owned(),
                    arabic: "الفاتحة".to_owned(),
                    verses: 3,
                },
            ),
            (
                2,
                chapters::Chapter {
                    name: "Al-Baqarah".to_owned(),
                    arabic: "البقرة".to_owned(),
                    verses: 2,
                },
            ),
        ]
        .into_iter()
        .collect()
    }

    fn plan(
        target_surah: Option<u16>,
        next_verse: Option<(u16, u16)>,
        daily_target: u16,
    ) -> db::Plan {
        db::Plan {
            target_surah,
            next_verse,
            daily_target,
        }
    }

    fn done(verses: &[&str]) -> Vec<String> {
        verses.iter().map(|verse| (*verse).to_owned()).collect()
    }

    #[test]
    fn reminder_times_and_offsets_are_validated() {
        assert_eq!(parse_time("19:30").as_deref(), Some("19:30"));
        assert_eq!(parse_time("9:05").as_deref(), Some("09:05"));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("19:60"), None);
        assert_eq!(parse_time("19"), None);

        assert_eq!(parse_offset("+03:00"), Some(180));
        assert_eq!(parse_offset("+05:30"), Some(330));
        assert_eq!(parse_offset("-04:00"), Some(-240));
        assert_eq!(parse_offset("+14:30"), None);
        assert_eq!(parse_offset("03:00"), None);

        assert_eq!(format_offset(0), "UTC+00:00");
        assert_eq!(format_offset(330), "UTC+05:30");
        assert_eq!(format_offset(-240), "UTC-04:00");
    }

    #[test]
    fn todays_wird_contains_the_daily_target_and_respects_plan_boundaries() {
        let chapters = fixture();

        assert_eq!(
            ayahs_to_show(&chapters, plan(None, Some((1, 1)), 1), &[]),
            vec!["1:1".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(
                &chapters,
                plan(None, Some((1, 3)), 3),
                &done(&["1:1", "1:2"])
            ),
            vec!["1:3".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(
                &chapters,
                plan(None, Some((2, 1)), 3),
                &done(&["1:1", "1:2", "1:3"])
            ),
            vec!["1:1".to_owned(), "1:2".to_owned(), "1:3".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(None, Some((1, 1)), 3), &[]),
            vec!["1:1".to_owned(), "1:2".to_owned(), "1:3".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(None, Some((1, 2)), 3), &[]),
            vec!["1:2".to_owned(), "1:3".to_owned(), "2:1".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(Some(1), Some((1, 2)), 3), &[]),
            vec!["1:2".to_owned(), "1:3".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(None, None, 1), &[]),
            Vec::<String>::new()
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(Some(1), None, 1), &done(&["1:3"])),
            vec!["1:3".to_owned()]
        );
        assert_eq!(
            ayahs_to_show(&chapters, plan(Some(1), None, 1), &done(&["1:2"])),
            vec!["1:2".to_owned()]
        );
    }
}
