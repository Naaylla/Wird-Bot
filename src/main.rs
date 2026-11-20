mod bot;
mod chapters;
mod commands;
mod components;
mod data;
mod db;
mod quran;
mod reminders;
mod ui;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use serenity::all::GatewayIntents;
use serenity::Client;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::chapters::Chapters;
use crate::data::Data;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("wird=info,serenity=warn")),
        )
        .init();

    tokio::spawn(keep_alive());

    let token = std::env::var("DISCORD_TOKEN").context(
        "DISCORD_TOKEN is not set. Copy .env.example to .env and put your bot token there.",
    )?;

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "wird.db".to_owned());
    warn_if_ephemeral_database(&db_path);
    let db =
        db::open(&db_path).with_context(|| format!("could not open the database at {db_path}"))?;
    info!("database ready at {db_path}");
    if let Some(offset) = std::env::var("DEFAULT_OFFSET")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        info!("default timezone offset: {offset}");
    } else {
        warn!(
            "DEFAULT_OFFSET is not set; days roll over at UTC midnight. Set e.g. DEFAULT_OFFSET=+01:00"
        );
    }
    let api = quran::QuranClient::new()?;

    let chapters = Chapters::default();
    if let Err(err) = chapters.load(&api).await {
        warn!("could not load surah names, retrying in the background: {err:#}");
    }
    tokio::spawn(refresh_chapters(api.clone(), chapters.clone()));

    let data = Arc::new(Data {
        db: Arc::new(Mutex::new(db)),
        api,
        chapters,
    });

    let mut client = Client::builder(&token, GatewayIntents::GUILDS)
        .event_handler(bot::Handler)
        .await
        .context("could not create the Discord client")?;

    client.data.write().await.insert::<Data>(data.clone());
    tokio::spawn(reminders::run(client.http.clone(), data.db.clone()));

    info!("Wird is online");
    client.start().await.context("the Discord client stopped")?;

    Ok(())
}

fn warn_if_ephemeral_database(db_path: &str) {
    let on_render = std::env::var_os("RENDER").is_some();
    let looks_persistent = db_path.starts_with("/var/data")
        || db_path.starts_with("/data")
        || std::env::var_os("RENDER_DISK_PATH").is_some();
    if on_render && !looks_persistent {
        warn!(
            "DATABASE_PATH={db_path} is not on a Render persistent disk. Progress will be wiped on every deploy. Mount a disk and set DATABASE_PATH=/var/data/wird.db"
        );
    }
}

async fn refresh_chapters(api: quran::QuranClient, chapters: Chapters) {
    loop {
        let delay = if chapters.is_empty().await {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(24 * 60 * 60)
        };
        tokio::time::sleep(delay).await;

        if let Err(err) = chapters.load(&api).await {
            warn!("could not refresh surah names: {err:#}");
        }
    }
}

async fn keep_alive() {
    let port = match std::env::var("PORT") {
        Ok(value) => match value.parse::<u16>() {
            Ok(port) => port,
            Err(err) => {
                error!("PORT must be a valid port number: {err}");
                return;
            }
        },
        Err(std::env::VarError::NotPresent) => 10000,
        Err(err) => {
            error!("could not read PORT: {err}");
            return;
        }
    };

    let listener = match TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(err) => {
            error!("could not bind keep-alive server on port {port}: {err}");
            return;
        }
    };
    info!("keep-alive server listening on 0.0.0.0:{port}");

    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(connection) => connection,
            Err(err) => {
                error!("keep-alive server could not accept a connection: {err}");
                return;
            }
        };

        if let Err(err) = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
        {
            warn!("could not reply to keep-alive request: {err}");
        }
    }
}
