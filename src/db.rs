use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{Days, NaiveDate};
use rusqlite::{params, Connection};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memorizations (
    user_id    TEXT NOT NULL,
    day        TEXT NOT NULL,
    verse_key  TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, day)
);
";

pub struct Stats {
    pub total: u32,
    pub streak: u32,
    pub done_today: bool,
}

pub struct LeaderEntry {
    pub user_id: u64,
    pub total: u32,
    pub streak: u32,
}

pub fn open(path: &str) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(SCHEMA)?;
    Ok(connection)
}

fn lock(db: &Arc<Mutex<Connection>>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub async fn is_done(db: Arc<Mutex<Connection>>, user_id: u64, day: &str) -> Result<bool> {
    let day = day.to_owned();

    tokio::task::spawn_blocking(move || -> Result<bool> {
        let connection = lock(&db);
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM memorizations WHERE user_id = ?1 AND day = ?2",
            params![user_id.to_string(), day],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    })
    .await?
}

pub async fn mark_done(
    db: Arc<Mutex<Connection>>,
    user_id: u64,
    day: &str,
    verse_key: &str,
) -> Result<bool> {
    let day = day.to_owned();
    let verse_key = verse_key.to_owned();

    tokio::task::spawn_blocking(move || -> Result<bool> {
        let connection = lock(&db);
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO memorizations (user_id, day, verse_key) VALUES (?1, ?2, ?3)",
            params![user_id.to_string(), day, verse_key],
        )?;
        Ok(inserted == 1)
    })
    .await?
}

pub async fn stats(db: Arc<Mutex<Connection>>, user_id: u64, today: NaiveDate) -> Result<Stats> {
    tokio::task::spawn_blocking(move || -> Result<Stats> {
        let connection = lock(&db);
        let mut statement =
            connection.prepare("SELECT day FROM memorizations WHERE user_id = ?1")?;
        let days: Vec<NaiveDate> = statement
            .query_map(params![user_id.to_string()], |row| row.get::<_, String>(0))?
            .filter_map(|row| row.ok())
            .filter_map(|day| NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok())
            .collect();

        let known: HashSet<NaiveDate> = days.iter().copied().collect();
        let (streak, done_today) = active_streak(&known, today);

        Ok(Stats {
            total: days.len() as u32,
            streak,
            done_today,
        })
    })
    .await?
}

pub async fn leaderboard(
    db: Arc<Mutex<Connection>>,
    today: NaiveDate,
    limit: usize,
) -> Result<Vec<LeaderEntry>> {
    tokio::task::spawn_blocking(move || -> Result<Vec<LeaderEntry>> {
        let connection = lock(&db);
        let mut statement = connection.prepare("SELECT user_id, day FROM memorizations")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut by_user: HashMap<u64, Vec<NaiveDate>> = HashMap::new();
        for row in rows {
            let (user_id, day) = row?;
            let (Ok(user_id), Ok(day)) = (
                user_id.parse::<u64>(),
                NaiveDate::parse_from_str(&day, "%Y-%m-%d"),
            ) else {
                continue;
            };
            by_user.entry(user_id).or_default().push(day);
        }

        let mut entries: Vec<LeaderEntry> = by_user
            .into_iter()
            .map(|(user_id, days)| {
                let known: HashSet<NaiveDate> = days.iter().copied().collect();
                let (streak, _) = active_streak(&known, today);
                LeaderEntry {
                    user_id,
                    total: days.len() as u32,
                    streak,
                }
            })
            .collect();

        entries.sort_by(|a, b| {
            b.streak
                .cmp(&a.streak)
                .then(b.total.cmp(&a.total))
                .then(a.user_id.cmp(&b.user_id))
        });
        entries.truncate(limit);

        Ok(entries)
    })
    .await?
}

fn active_streak(known: &HashSet<NaiveDate>, today: NaiveDate) -> (u32, bool) {
    let done_today = known.contains(&today);
    let mut cursor = if done_today {
        today
    } else {
        today - Days::new(1)
    };
    let mut streak: u32 = 0;

    while known.contains(&cursor) {
        streak += 1;
        cursor = cursor - Days::new(1);
    }

    (streak, done_today)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn a_user_can_only_memorize_one_ayah_per_day() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wird-test-{unique}.db"));
        let connection = open(path.to_str().unwrap()).unwrap();
        let db = Arc::new(Mutex::new(connection));

        assert!(!is_done(db.clone(), 1, "2026-11-12").await.unwrap());
        assert!(mark_done(db.clone(), 1, "2026-11-12", "2:255")
            .await
            .unwrap());
        assert!(!mark_done(db.clone(), 1, "2026-11-12", "3:1").await.unwrap());
        assert!(is_done(db.clone(), 1, "2026-11-12").await.unwrap());
        assert!(!is_done(db.clone(), 1, "2026-11-13").await.unwrap());

        let stats = stats(db, 1, NaiveDate::from_ymd_opt(2026, 11, 12).unwrap())
            .await
            .unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.streak, 1);
        assert!(stats.done_today);

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn leaderboard_ranks_by_current_streak() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wird-leader-test-{unique}.db"));
        let connection = open(path.to_str().unwrap()).unwrap();
        let db = Arc::new(Mutex::new(connection));

        for day in ["2026-11-10", "2026-11-11", "2026-11-12"] {
            mark_done(db.clone(), 1, day, "2:255").await.unwrap();
        }
        mark_done(db.clone(), 2, "2026-11-12", "2:255")
            .await
            .unwrap();
        mark_done(db.clone(), 3, "2026-11-01", "2:255")
            .await
            .unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let entries = leaderboard(db.clone(), today, 10).await.unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].user_id, 1);
        assert_eq!(entries[0].streak, 3);
        assert_eq!(entries[0].total, 3);
        assert_eq!(entries[1].user_id, 2);
        assert_eq!(entries[1].streak, 1);
        assert_eq!(entries[2].user_id, 3);
        assert_eq!(entries[2].streak, 0);

        let top = leaderboard(db, today, 1).await.unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].user_id, 1);

        let _ = std::fs::remove_file(path);
    }
}
