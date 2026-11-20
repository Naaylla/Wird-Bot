use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::quran::QuranClient;

pub type ChapterList = BTreeMap<u16, Chapter>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    pub name: String,
    pub arabic: String,
    pub verses: u16,
}

#[derive(Clone, Default)]
pub struct Chapters {
    inner: Arc<RwLock<Arc<ChapterList>>>,
}

impl Chapters {
    pub async fn load(&self, api: &QuranClient) -> Result<()> {
        let chapters = api.chapters().await?;
        *self.inner.write().await = Arc::new(chapters);
        Ok(())
    }

    pub async fn snapshot(&self) -> Arc<ChapterList> {
        self.inner.read().await.clone()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

pub fn verse_count(chapters: &ChapterList, surah: u16) -> Option<u16> {
    chapters.get(&surah).map(|chapter| chapter.verses)
}

pub fn next_verse(
    chapters: &ChapterList,
    current: (u16, u16),
    target_surah: Option<u16>,
) -> Option<(u16, u16)> {
    let (surah, ayah) = current;
    let count = verse_count(chapters, surah)?;

    if ayah == 0 || ayah > count || target_surah.is_some_and(|target| target != surah) {
        return None;
    }

    if ayah < count {
        Some((surah, ayah + 1))
    } else if target_surah.is_none() {
        chapters
            .range(surah.saturating_add(1)..)
            .next()
            .map(|(next_surah, _)| (*next_surah, 1))
    } else {
        None
    }
}

#[cfg(test)]
pub fn final_verse(chapters: &ChapterList, target_surah: Option<u16>) -> Option<(u16, u16)> {
    let surah = match target_surah {
        Some(surah) => surah,
        None => *chapters.keys().next_back()?,
    };

    verse_count(chapters, surah).map(|last_ayah| (surah, last_ayah))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ChapterList {
        [(1, 7), (2, 286), (9, 129), (10, 109), (114, 6)]
            .into_iter()
            .map(|(surah, verses)| {
                (
                    surah,
                    Chapter {
                        name: format!("Surah {surah}"),
                        arabic: format!("سورة {surah}"),
                        verses,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn sequence_advances_in_quran_order_and_stops_at_boundaries() {
        let chapters = fixture();
        assert_eq!(verse_count(&chapters, 0), None);
        assert_eq!(verse_count(&chapters, 1), Some(7));
        assert_eq!(next_verse(&chapters, (1, 1), None), Some((1, 2)));
        assert_eq!(next_verse(&chapters, (1, 7), None), Some((2, 1)));
        assert_eq!(next_verse(&chapters, (1, 7), Some(1)), None);
        assert_eq!(next_verse(&chapters, (114, 6), None), None);
        assert_eq!(final_verse(&chapters, Some(10)), Some((10, 109)));
        assert_eq!(final_verse(&chapters, None), Some((114, 6)));
    }
}
