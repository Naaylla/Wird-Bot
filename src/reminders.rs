use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serenity::all::{CreateMessage, GuildId, Http, UserId};
use tracing::warn;

use crate::db;

const INTERVAL: Duration = Duration::from_secs(60);

pub async fn run(http: Arc<Http>, db: Arc<Mutex<Connection>>) {
    loop {
        tokio::time::sleep(INTERVAL).await;

        if let Err(err) = tick(&http, db.clone()).await {
            warn!("could not check daily reminders: {err:#}");
        }
    }
}

async fn tick(http: &Http, db: Arc<Mutex<Connection>>) -> Result<()> {
    for due in db::due_reminders(db.clone(), Utc::now()).await? {
        let reminder = due.reminder;

        // Mark first so a user whose DMs are closed is not retried every minute.
        if let Err(err) =
            db::mark_reminder_sent(db.clone(), reminder.guild_id, reminder.user_id, &due.day).await
        {
            warn!(
                "could not record a reminder for user {}: {err:#}",
                reminder.user_id
            );
            continue;
        }

        if let Err(err) = notify(http, db.clone(), &reminder).await {
            warn!(
                "could not send a reminder to user {}: {err:#}",
                reminder.user_id
            );
        }
    }

    Ok(())
}

async fn notify(http: &Http, db: Arc<Mutex<Connection>>, reminder: &db::Reminder) -> Result<()> {
    let Some(plan) = db::get_plan(db.clone(), reminder.guild_id, reminder.user_id).await? else {
        return Ok(());
    };

    let day = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let completed = db::verses_for_day(db.clone(), reminder.guild_id, reminder.user_id, &day)
        .await?
        .len() as u16;
    if completed >= plan.daily_target {
        return Ok(());
    }

    let server = match http.get_guild(GuildId::new(reminder.guild_id)).await {
        Ok(guild) => guild.name,
        Err(_) => "your server".to_owned(),
    };

    let content = format!(
        "Assalamu alaikum! Your wird in **{server}** is not finished yet — {completed}/{} ayahs today.\nRun `/wird` to continue.",
        plan.daily_target
    );

    UserId::new(reminder.user_id)
        .create_dm_channel(http)
        .await?
        .send_message(http, CreateMessage::new().content(content))
        .await?;

    Ok(())
}
