mod bot;
mod commands;
mod components;
mod data;
mod db;
mod quran;
mod ui;

use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use serenity::all::GatewayIntents;
use serenity::Client;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

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

    let db = db::open("wird.db").context("could not open the database")?;
    let api = quran::QuranClient::new()?;

    let chapters = match api.chapters().await {
        Ok(chapters) => chapters,
        Err(err) => {
            warn!("could not load surah names, falling back to numbers: {err:#}");
            Default::default()
        }
    };

    let data = Arc::new(Data {
        db: Arc::new(Mutex::new(db)),
        api,
        chapters,
    });

    let mut client = Client::builder(&token, GatewayIntents::GUILDS)
        .event_handler(bot::Handler)
        .await
        .context("could not create the Discord client")?;

    client.data.write().await.insert::<Data>(data);

    info!("Wird is online");
    client.start().await.context("the Discord client stopped")?;

    Ok(())
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
