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

        let text = response.tafsir.text;

        if tafsir_id == TAFSIR_EN_ID {
            if let Some(section) = self.ayah_only_section(verse_key, &text).await {
                return Ok(section);
            }
        }

        Ok(text)
    }

    async fn ayah_only_section(&self, verse_key: &str, text: &str) -> Option<String> {
        let arabic = self.ayah_arabic(verse_key).await.ok()?;
        let (surah, ayah) = verse_parts(verse_key);
        let next_arabic = self
            .ayah_arabic(&format!("{surah}:{}", ayah + 1))
            .await
            .ok();

        extract_ayah_section(text, &arabic, next_arabic.as_deref())
    }

    async fn ayah_arabic(&self, verse_key: &str) -> Result<String> {
        let response: VerseResponse = self
            .http
            .get(format!(
                "{API_BASE}/verses/by_key/{verse_key}?fields=text_uthmani"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.verse.text_uthmani)
    }
}

fn extract_ayah_section(text: &str, arabic: &str, next_arabic: Option<&str>) -> Option<String> {
    let blocks = html_blocks(text);
    let target = normalize_arabic(&strip_html(arabic));
    if target.is_empty() {
        return None;
    }

    let start = blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| is_arabic_only(block))
        .filter_map(|(index, block)| {
            arabic_distance(block, &target).map(|distance| (index, distance))
        })
        .min_by_key(|(index, distance)| (*distance, *index))
        .map(|(index, _)| index)?;

    let next = next_arabic.map(|arabic| normalize_arabic(&strip_html(arabic)));
    let end = next
        .as_deref()
        .and_then(|next| {
            blocks
                .iter()
                .enumerate()
                .skip(start + 1)
                .find(|(_, block)| is_arabic_only(block) && arabic_distance(block, next).is_some())
                .map(|(index, _)| index)
        })
        .or_else(|| {
            let mut seen_text = false;
            blocks
                .iter()
                .enumerate()
                .skip(start + 1)
                .find(|(_, block)| {
                    if block.chars().any(|c| c.is_ascii_alphabetic()) {
                        seen_text = true;
                        false
                    } else {
                        seen_text && is_arabic_only(block)
                    }
                })
                .map(|(index, _)| index)
        })
        .unwrap_or(blocks.len());

    if end <= start + 1 {
        return None;
    }

    Some(blocks[start + 1..end].join("\n\n"))
}

fn html_blocks(html: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut position = 0;

    while position < html.len() {
        let Some(open_offset) = html[position..].find('<') else {
            break;
        };
        let open = position + open_offset;
        let Some(open_end_offset) = html[open..].find('>') else {
            break;
        };
        let open_end = open + open_end_offset;
        let tag = &html[open + 1..open_end];
        let name = tag.split_whitespace().next().unwrap_or("");

        if name == "p" || name == "h2" {
            let close = format!("</{name}>");
            if let Some(close_offset) = html[open_end + 1..].find(&close) {
                let close_start = open_end + 1 + close_offset;
                blocks.push(html[open_end + 1..close_start].to_owned());
                position = close_start + close.len();
                continue;
            }
        }

        position = open_end + 1;
    }

    blocks
}

fn strip_html(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;

    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }

    output
}

fn is_arabic_only(block: &str) -> bool {
    let text = strip_html(block);
    !text.is_empty() && !text.chars().any(|c| c.is_ascii_alphabetic())
}

fn arabic_distance(block: &str, target: &str) -> Option<usize> {
    let normalized = normalize_arabic(&strip_html(block));

    if normalized == target {
        return Some(0);
    }

    if normalized.starts_with(target) || target.starts_with(&normalized) {
        return Some(normalized.len().abs_diff(target.len()));
    }

    None
}

fn normalize_arabic(text: &str) -> String {
    text.chars()
        .filter(|character| !is_arabic_mark(*character))
        .filter(|character| !character.is_whitespace())
        .map(|character| match character {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ى' => 'ي',
            'ة' => 'ه',
            _ => character,
        })
        .collect()
}

fn is_arabic_mark(character: char) -> bool {
    matches!(
        character as u32,
        0x064B..=0x065F | 0x0670 | 0x06D6..=0x06ED | 0x0640
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    const GROUPED_TAFSIR: &str = "<h2>Which was Revealed in Makkah</h2>\
        <p>بِسْمِ اللَّهِ الرَّحْمَنِ الرَّحِيمِ</p>\
        <p>قَدْ أَفْلَحَ الْمُؤْمِنُونَ</p>\
        <p>Commentary for the first ayah.</p>\
        <p>وَٱلَّذِينَ هُمْ لِلزَّكَاةِ فَاعِلُونَ</p>\
        <p>Commentary for the fourth ayah with a <span>quote</span>.</p>\
        <p>وَالَّذِينَ هُمْ لِفُرُوجِهِمْ حَافِظُونَ</p>\
        <p>Commentary for the fifth ayah.</p>";

    #[test]
    fn grouped_tafsir_is_trimmed_to_one_ayah() {
        let section = extract_ayah_section(
            GROUPED_TAFSIR,
            "وَٱلَّذِينَ هُمْ لِلزَّكَاةِ فَاعِلُونَ",
            Some("وَالَّذِينَ هُمْ لِفُرُوجِهِمْ حَافِظُونَ"),
        )
        .unwrap();

        assert!(section.contains("Commentary for the fourth ayah"));
        assert!(!section.contains("Commentary for the first ayah"));
        assert!(!section.contains("Commentary for the fifth ayah"));
    }

    #[test]
    fn alef_and_diacritic_variants_still_match() {
        assert_eq!(normalize_arabic("وَٱلَّذِينَ"), normalize_arabic("والذين"));
    }

    #[test]
    fn per_ayah_tafsir_is_left_unchanged() {
        let per_ayah = "<p>This tafsir is already only about one ayah.</p>";
        assert!(extract_ayah_section(per_ayah, "قَدْ أَفْلَحَ الْمُؤْمِنُونَ", None).is_none());
    }
}
