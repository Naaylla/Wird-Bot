use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

const API_BASE: &str = "https://api.quran.com/api/v4";
const TRANSLATION_ID: u32 = 20;
pub const TAFSIR_EN_ID: u32 = 169;
pub const TAFSIR_AR_ID: u32 = 16;

const SURAH_LENGTHS: [u16; 114] = [
    7, 286, 200, 176, 120, 165, 206, 75, 129, 109, 123, 111, 43, 52, 99, 128, 111, 110, 98, 135,
    112, 78, 118, 64, 77, 227, 93, 88, 69, 60, 34, 30, 73, 54, 45, 83, 182, 88, 75, 85, 54, 53, 89,
    59, 37, 35, 38, 29, 18, 45, 60, 49, 62, 55, 78, 96, 29, 22, 24, 13, 14, 11, 11, 18, 12, 12, 30,
    52, 52, 44, 28, 28, 20, 56, 40, 31, 50, 40, 46, 42, 29, 19, 36, 25, 22, 17, 19, 26, 30, 20, 15,
    21, 11, 8, 8, 19, 5, 8, 8, 11, 11, 8, 3, 9, 5, 4, 7, 3, 6, 3, 5, 4, 5, 6,
];

#[derive(Clone)]
pub struct Chapter {
    pub name: String,
    pub arabic: String,
}

pub struct Ayah {
    pub verse_key: String,
    pub arabic: String,
    pub translation: String,
}

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

    pub async fn chapters(&self) -> Result<HashMap<u16, Chapter>> {
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

pub fn chapter_ayah_count(surah: u16) -> Option<u16> {
    surah
        .checked_sub(1)
        .and_then(|index| SURAH_LENGTHS.get(usize::from(index)))
        .copied()
}

pub fn next_verse(current: (u16, u16), target_surah: Option<u16>) -> Option<(u16, u16)> {
    let (surah, ayah) = current;
    let count = chapter_ayah_count(surah)?;

    if ayah == 0 || ayah > count || target_surah.is_some_and(|target| target != surah) {
        return None;
    }

    if ayah < count {
        Some((surah, ayah + 1))
    } else if target_surah.is_none() && surah < SURAH_LENGTHS.len() as u16 {
        Some((surah + 1, 1))
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surah_lengths_add_up_to_the_whole_quran() {
        let total: u32 = SURAH_LENGTHS.iter().map(|length| u32::from(*length)).sum();
        assert_eq!(total, 6236);
    }

    #[test]
    fn sequence_advances_in_quran_order_and_stops_at_boundaries() {
        assert_eq!(chapter_ayah_count(0), None);
        assert_eq!(chapter_ayah_count(1), Some(7));
        assert_eq!(next_verse((1, 1), None), Some((1, 2)));
        assert_eq!(next_verse((1, 7), None), Some((2, 1)));
        assert_eq!(next_verse((1, 7), Some(1)), None);
        assert_eq!(next_verse((114, 6), None), None);
    }
}
