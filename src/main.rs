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
use tracing::{info, warn};

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
