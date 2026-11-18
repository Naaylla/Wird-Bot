use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::chapters::{Chapter, ChapterList};

const API_BASE: &str = "https://api.quran.com/api/v4";
const TRANSLATION_ID: u32 = 20;
pub const TAFSIR_EN_ID: u32 = 169;
pub const TAFSIR_AR_ID: u32 = 16;

pub struct Ayah {
    pub verse_key: String,
    pub arabic: String,
    pub translation: String,
}

#[derive(Clone)]
pub struct QuranClient {
    http: Client,
}

impl QuranClient {
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!("Wird/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .context("could not create the HTTP client")?;

        Ok(Self { http })
    }

    pub async fn chapters(&self) -> Result<ChapterList> {
        let response: ChaptersResponse = self
            .http
            .get(format!("{API_BASE}/chapters?language=en"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response
            .chapters
            .into_iter()
            .map(|chapter| {
                (
                    chapter.id,
                    Chapter {
                        name: chapter.name_simple,
                        arabic: chapter.name_arabic,
                        verses: chapter.verses_count,
                    },
                )
            })
            .collect())
    }

    pub async fn ayah(&self, verse_key: &str) -> Result<Ayah> {
        let response: VerseResponse = self
            .http
            .get(format!(
                "{API_BASE}/verses/by_key/{verse_key}?translations={TRANSLATION_ID}&fields=text_uthmani"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let verse = response.verse;
        let translation = verse
            .translations
            .into_iter()
            .next()
            .map(|translation| translation.text)
            .unwrap_or_default();

        Ok(Ayah {
            verse_key: verse.verse_key,
            arabic: verse.text_uthmani,
            translation,
        })
    }

    pub async fn tafsir(&self, tafsir_id: u32, verse_key: &str) -> Result<String> {
        let response: TafsirResponse = self
            .http
            .get(format!(
                "{API_BASE}/tafsirs/{tafsir_id}/by_ayah/{verse_key}"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.tafsir.text)
    }
}

pub fn verse_parts(verse_key: &str) -> (u16, u16) {
    let mut parts = verse_key.split(':');
    let surah = parts.next().and_then(|part| part.parse().ok()).unwrap_or(1);
    let ayah = parts.next().and_then(|part| part.parse().ok()).unwrap_or(1);
    (surah, ayah)
}

#[derive(Deserialize)]
struct ChaptersResponse {
    chapters: Vec<ChapterResponse>,
}

#[derive(Deserialize)]
struct ChapterResponse {
    id: u16,
    name_simple: String,
    name_arabic: String,
    verses_count: u16,
}

#[derive(Deserialize)]
struct VerseResponse {
    verse: Verse,
}

#[derive(Deserialize)]
struct Verse {
    verse_key: String,
    text_uthmani: String,
    #[serde(default)]
    translations: Vec<Translation>,
}

#[derive(Deserialize)]
struct Translation {
    text: String,
}

#[derive(Deserialize)]
struct TafsirResponse {
    tafsir: Tafsir,
}

#[derive(Deserialize)]
struct Tafsir {
    text: String,
}
