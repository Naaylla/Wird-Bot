use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serenity::prelude::{Context as SerenityContext, TypeMapKey};

use crate::quran::{Chapter, QuranClient};

pub struct Data {
    pub db: Arc<Mutex<Connection>>,
    pub api: QuranClient,
    pub chapters: HashMap<u16, Chapter>,
}

impl TypeMapKey for Data {
    type Value = Arc<Data>;
}

impl Data {
    pub async fn from_context(ctx: &SerenityContext) -> Result<Arc<Self>> {
        ctx.data
            .read()
            .await
            .get::<Self>()
            .cloned()
            .context("bot data is not initialized")
    }
}
