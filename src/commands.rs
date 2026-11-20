use anyhow::Result;
use chrono::Utc;
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, Context, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse, GuildId,
};
use tracing::warn;

use crate::data::Data;
use crate::db;
use crate::quran;
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

    let offset = db::get_timezone(data.db.clone(), guild_id, user_id).await?;
    let day = db::local_date(offset);
    let day_string = day.format("%Y-%m-%d").to_string();
    let completed_today =
        db::verses_for_day(data.db.clone(), guild_id, user_id, &day_string).await?;
    let completed = completed_today.len() as u16;
    let target = plan.daily_target;
    let next_wird = if completed >= target {
        Some(db::next_wird_mention(db::next_wird_at(Utc::now(), offset)))
    } else {
        None
    };

    let Some(verse_key) = ayah_to_show(plan, &completed_today) else {
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
    let done = completed_today.contains(&verse_key);
    let blocked_today = completed >= target && !done;
    let (surah, _) = quran::verse_parts(&ayah.verse_key);

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new()
                .embed(ui::daily_embed(
                    &ayah,
                    chapters.get(&surah),
                    completed,
                    target,
                    done,
                    plan.next_verse.is_none(),
                    next_wird.as_deref(),
                ))
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
    let user_id = command.user.id.get();
    let offset = db::get_timezone(data.db.clone(), guild_id, user_id).await?;
    let today = db::local_date(offset);
    let entries = db::leaderboard(data.db.clone(), guild_id, Utc::now(), 15).await?;

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
    let offset = db::get_timezone(data.db.clone(), guild_id, user_id).await?;
    let today = db::local_date(offset);
    let stats = db::stats(data.db.clone(), guild_id, user_id, today).await?;
    let next_wird = if stats.done_today {
        Some(db::next_wird_mention(db::next_wird_at(Utc::now(), offset)))
    } else {
        None
    };

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new().embed(ui::progress_embed(
                stats.total,
                stats.streak,
                stats.done_today,
                next_wird.as_deref(),
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
        Some(raw) => match db::parse_offset(raw) {
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
        None => db::get_timezone(data.db.clone(), guild_id, user_id).await?,
    };

    db::set_reminder(data.db.clone(), guild_id, user_id, &time, offset).await?;
    db::set_timezone(data.db.clone(), guild_id, user_id, offset).await?;

    let message = format!(
        "Reminder set for **{time}** ({}). I will DM you when that day's wird is unfinished.",
        format_offset(offset)
    );
    reply_ephemeral(ctx, command, &message).await
}

pub async fn timezone(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let Some(guild_id) = guild_id(command) else {
        return reply_ephemeral(ctx, command, "Use this command in a server.").await;
    };

    let data = Data::from_context(ctx).await?;
    let user_id = command.user.id.get();

    let Some(raw_offset) = option_string(command, "offset") else {
        let offset = db::get_timezone(data.db.clone(), guild_id, user_id).await?;
        let next = db::next_wird_mention(db::next_wird_at(Utc::now(), offset));
        let message = format!(
            "Your timezone is **{}**. Each day's wird starts at midnight there. {next}. Change it with `/timezone offset:+01:00`.",
            format_offset(offset)
        );
        return reply_ephemeral(ctx, command, &message).await;
    };

    let Some(offset) = db::parse_offset(raw_offset) else {
        return reply_ephemeral(
            ctx,
            command,
            "Use a UTC offset like `+03:00`, `+05:30` or `-04:00`.",
        )
        .await;
    };

    db::set_timezone(data.db.clone(), guild_id, user_id, offset).await?;

    let next = db::next_wird_mention(db::next_wird_at(Utc::now(), offset));
    let message = format!(
        "Your timezone is now **{}**. Each day's wird starts at midnight there. {next}.",
        format_offset(offset)
    );
    reply_ephemeral(ctx, command, &message).await
}

pub async fn usage(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let owner = std::env::var("OWNER_ID")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok());
    if owner != Some(command.user.id.get()) {
        return reply_ephemeral(ctx, command, "This command is for the bot owner.").await;
    }

    command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

    let data = Data::from_context(ctx).await?;
    let guilds = db::usage(data.db.clone()).await?;

    let mut names = std::collections::HashMap::new();
    for guild in &guilds {
        if let Ok(partial) = ctx.http.get_guild(GuildId::new(guild.guild_id)).await {
            names.insert(guild.guild_id, partial.name);
        }
    }

    command
        .edit_response(
            ctx.http.clone(),
            EditInteractionResponse::new().embed(ui::usage_embed(&guilds, &names)),
        )
        .await?;

    Ok(())
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

fn ayah_to_show(plan: db::Plan, completed_today: &[String]) -> Option<String> {
    if completed_today.len() as u16 >= plan.daily_target || plan.next_verse.is_none() {
        return completed_today.last().cloned();
    }

    plan.next_verse
        .map(|(surah, ayah)| format!("{surah}:{ayah}"))
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

        assert_eq!(db::parse_offset("+03:00"), Some(180));
        assert_eq!(db::parse_offset("+05:30"), Some(330));
        assert_eq!(db::parse_offset("-04:00"), Some(-240));
        assert_eq!(db::parse_offset("+14:30"), None);
        assert_eq!(db::parse_offset("03:00"), None);

        assert_eq!(format_offset(0), "UTC+00:00");
        assert_eq!(format_offset(330), "UTC+05:30");
        assert_eq!(format_offset(-240), "UTC-04:00");
    }

    #[test]
    fn todays_wird_shows_one_ayah_and_advances_after_memorization() {
        assert_eq!(
            ayah_to_show(plan(None, Some((1, 1)), 3), &[]),
            Some("1:1".to_owned())
        );
        assert_eq!(
            ayah_to_show(plan(None, Some((1, 2)), 3), &done(&["1:1"])),
            Some("1:2".to_owned())
        );
        assert_eq!(
            ayah_to_show(plan(None, Some((2, 1)), 3), &done(&["1:1", "1:2", "1:3"])),
            Some("1:3".to_owned())
        );
        assert_eq!(ayah_to_show(plan(None, None, 1), &[]), None);
        assert_eq!(
            ayah_to_show(plan(Some(1), None, 3), &done(&["1:3"])),
            Some("1:3".to_owned())
        );
    }
}
