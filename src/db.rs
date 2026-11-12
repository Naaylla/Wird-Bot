use std::collections::HashSet;
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

        Ok(Stats {
            total: days.len() as u32,
            streak,
            done_today,
        })
    })
    .await?
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
}
