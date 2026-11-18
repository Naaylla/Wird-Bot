use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{Days, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS memorizations (
    guild_id   TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    day        TEXT NOT NULL,
    verse_key  TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (guild_id, user_id, day, verse_key)
);

CREATE TABLE IF NOT EXISTS user_plans (
    guild_id     TEXT NOT NULL,
    user_id      TEXT NOT NULL,
    target_surah INTEGER,
    next_surah   INTEGER,
    next_ayah    INTEGER,
    daily_target INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (guild_id, user_id),
    CHECK ((next_surah IS NULL) = (next_ayah IS NULL))
);
";

const LEGACY_GUILD_ID: &str = "legacy";
const SCHEMA_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub target_surah: Option<u16>,
    pub next_verse: Option<(u16, u16)>,
    pub daily_target: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkResult {
    Inserted,
    AlreadyDone,
    DailyLimit,
    Stale,
}

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
    let fresh = !table_exists(&connection, "user_plans")?;
    migrate(&connection)?;
    connection.execute_batch(SCHEMA)?;
    if fresh {
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    } else {
        upgrade(&connection)?;
    }
    Ok(connection)
}

fn upgrade(connection: &Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection.unchecked_transaction()?;

    if !column_exists(&transaction, "user_plans", "daily_target")? {
        transaction.execute_batch(
            "ALTER TABLE user_plans ADD COLUMN daily_target INTEGER NOT NULL DEFAULT 1",
        )?;
    }

    transaction.execute_batch(
        "ALTER TABLE memorizations RENAME TO memorizations_old;
         CREATE TABLE memorizations (
             guild_id   TEXT NOT NULL,
             user_id    TEXT NOT NULL,
             day        TEXT NOT NULL,
             verse_key  TEXT NOT NULL,
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             PRIMARY KEY (guild_id, user_id, day, verse_key)
         );
         INSERT OR IGNORE INTO memorizations (guild_id, user_id, day, verse_key, created_at)
             SELECT guild_id, user_id, day, verse_key, created_at FROM memorizations_old;
         DROP TABLE memorizations_old;
         PRAGMA user_version = 2;",
    )?;

    transaction.commit()?;
    Ok(())
}

fn migrate(connection: &Connection) -> Result<()> {
    let plans_need_migration = table_exists(connection, "user_plans")?
        && !column_exists(connection, "user_plans", "guild_id")?;
    let memorizations_need_migration = table_exists(connection, "memorizations")?
        && !column_exists(connection, "memorizations", "guild_id")?;

    if !plans_need_migration && !memorizations_need_migration {
        return Ok(());
    }

    let transaction = connection.unchecked_transaction()?;

    if plans_need_migration {
        transaction.execute_batch("ALTER TABLE user_plans RENAME TO user_plans_legacy")?;
    }
    if memorizations_need_migration {
        transaction.execute_batch("ALTER TABLE memorizations RENAME TO memorizations_legacy")?;
    }

    transaction.execute_batch(SCHEMA)?;

    if plans_need_migration {
        transaction.execute(
            "INSERT INTO user_plans (guild_id, user_id, target_surah, next_surah, next_ayah)
             SELECT ?1, user_id, target_surah, next_surah, next_ayah FROM user_plans_legacy",
            params![LEGACY_GUILD_ID],
        )?;
        transaction.execute_batch("DROP TABLE user_plans_legacy")?;
    }
    if memorizations_need_migration {
        transaction.execute(
            "INSERT INTO memorizations (guild_id, user_id, day, verse_key, created_at)
             SELECT ?1, user_id, day, verse_key, created_at FROM memorizations_legacy",
            params![LEGACY_GUILD_ID],
        )?;
        transaction.execute_batch("DROP TABLE memorizations_legacy")?;
    }

    transaction.commit()?;
    Ok(())
}

fn table_exists(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }

    Ok(false)
}

fn lock(db: &Arc<Mutex<Connection>>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_plan(
    connection: &Connection,
    guild_id: u64,
    user_id: u64,
) -> rusqlite::Result<Option<Plan>> {
    connection
        .query_row(
            "SELECT target_surah, next_surah, next_ayah, daily_target FROM user_plans
             WHERE guild_id = ?1 AND user_id = ?2",
            params![guild_id.to_string(), user_id.to_string()],
            |row| {
                let target_surah = row.get(0)?;
                let next_surah: Option<u16> = row.get(1)?;
                let next_ayah: Option<u16> = row.get(2)?;
                let daily_target = row.get(3)?;
                Ok(Plan {
                    target_surah,
                    next_verse: next_surah.zip(next_ayah),
                    daily_target,
                })
            },
        )
        .optional()
}

pub async fn get_plan(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    user_id: u64,
) -> Result<Option<Plan>> {
    tokio::task::spawn_blocking(move || {
        let connection = lock(&db);
        Ok(read_plan(&connection, guild_id, user_id)?)
    })
    .await?
}

pub async fn set_plan(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    user_id: u64,
    target_surah: Option<u16>,
    daily_target: Option<u16>,
) -> Result<Plan> {
    tokio::task::spawn_blocking(move || {
        let connection = lock(&db);
        let existing = read_plan(&connection, guild_id, user_id)?;
        let daily_target = daily_target
            .or(existing.map(|plan| plan.daily_target))
            .unwrap_or(1)
            .clamp(1, 10);

        if let Some(plan) = existing {
            if plan.target_surah == target_surah && plan.next_verse.is_some() {
                if plan.daily_target != daily_target {
                    connection.execute(
                        "UPDATE user_plans SET daily_target = ?3
                         WHERE guild_id = ?1 AND user_id = ?2",
                        params![guild_id.to_string(), user_id.to_string(), daily_target],
                    )?;
                    return Ok(Plan {
                        daily_target,
                        ..plan
                    });
                }
                return Ok(plan);
            }
        }

        let (surah, ayah) = (target_surah.unwrap_or(1), 1);
        connection.execute(
            "INSERT INTO user_plans
                 (guild_id, user_id, target_surah, next_surah, next_ayah, daily_target)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET
                 target_surah = excluded.target_surah,
                 next_surah = excluded.next_surah,
                 next_ayah = excluded.next_ayah,
                 daily_target = excluded.daily_target",
            params![
                guild_id.to_string(),
                user_id.to_string(),
                target_surah,
                surah,
                ayah,
                daily_target
            ],
        )?;

        Ok(Plan {
            target_surah,
            next_verse: Some((surah, ayah)),
            daily_target,
        })
    })
    .await?
}

pub async fn verses_for_day(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    user_id: u64,
    day: &str,
) -> Result<Vec<String>> {
    let day = day.to_owned();

    tokio::task::spawn_blocking(move || {
        let connection = lock(&db);
        let mut statement = connection.prepare(
            "SELECT verse_key FROM memorizations
             WHERE guild_id = ?1 AND user_id = ?2 AND day = ?3
             ORDER BY rowid",
        )?;
        let rows = statement.query_map(
            params![guild_id.to_string(), user_id.to_string(), day],
            |row| row.get(0),
        )?;

        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    })
    .await?
}

pub async fn mark_done(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    user_id: u64,
    day: &str,
    verse_key: &str,
    expected_plan: Plan,
    next_verse: Option<(u16, u16)>,
) -> Result<MarkResult> {
    let day = day.to_owned();
    let verse_key = verse_key.to_owned();

    tokio::task::spawn_blocking(move || {
        let mut connection = lock(&db);
        let transaction = connection.transaction()?;

        let already_done: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM memorizations
             WHERE guild_id = ?1 AND user_id = ?2 AND day = ?3 AND verse_key = ?4)",
            params![guild_id.to_string(), user_id.to_string(), day, verse_key],
            |row| row.get(0),
        )?;
        if already_done {
            return Ok(MarkResult::AlreadyDone);
        }

        if read_plan(&transaction, guild_id, user_id)? != Some(expected_plan)
            || expected_plan
                .next_verse
                .map(|(surah, ayah)| format!("{surah}:{ayah}"))
                != Some(verse_key.clone())
        {
            return Ok(MarkResult::Stale);
        }

        let done_today: u16 = transaction.query_row(
            "SELECT COUNT(*) FROM memorizations
             WHERE guild_id = ?1 AND user_id = ?2 AND day = ?3",
            params![guild_id.to_string(), user_id.to_string(), day],
            |row| row.get(0),
        )?;
        if done_today >= expected_plan.daily_target {
            return Ok(MarkResult::DailyLimit);
        }

        transaction.execute(
            "INSERT INTO memorizations (guild_id, user_id, day, verse_key)
             VALUES (?1, ?2, ?3, ?4)",
            params![guild_id.to_string(), user_id.to_string(), day, verse_key],
        )?;
        transaction.execute(
            "UPDATE user_plans SET next_surah = ?3, next_ayah = ?4
             WHERE guild_id = ?1 AND user_id = ?2",
            params![
                guild_id.to_string(),
                user_id.to_string(),
                next_verse.map(|(surah, _)| surah),
                next_verse.map(|(_, ayah)| ayah)
            ],
        )?;
        transaction.commit()?;

        Ok(MarkResult::Inserted)
    })
    .await?
}

pub async fn reset(db: Arc<Mutex<Connection>>, guild_id: u64, user_id: u64) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut connection = lock(&db);
        let transaction = connection.transaction()?;

        transaction.execute(
            "DELETE FROM memorizations WHERE guild_id = ?1 AND user_id = ?2",
            params![guild_id.to_string(), user_id.to_string()],
        )?;
        transaction.execute(
            "UPDATE user_plans SET next_surah = COALESCE(target_surah, 1), next_ayah = 1
             WHERE guild_id = ?1 AND user_id = ?2",
            params![guild_id.to_string(), user_id.to_string()],
        )?;

        transaction.commit()?;
        Ok(())
    })
    .await?
}

pub async fn stats(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    user_id: u64,
    today: NaiveDate,
) -> Result<Stats> {
    tokio::task::spawn_blocking(move || {
        let connection = lock(&db);
        let mut statement = connection.prepare(
            "SELECT day, verse_key FROM memorizations WHERE guild_id = ?1 AND user_id = ?2",
        )?;
        let rows = statement
            .query_map(params![guild_id.to_string(), user_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;

        let mut days = Vec::new();
        let mut verses = HashSet::new();
        for row in rows {
            let (day, verse) = row?;
            if let Ok(day) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
                days.push(day);
                verses.insert(verse);
            }
        }

        let known: HashSet<NaiveDate> = days.into_iter().collect();
        let (streak, done_today) = active_streak(&known, today);

        Ok(Stats {
            total: verses.len() as u32,
            streak,
            done_today,
        })
    })
    .await?
}

pub async fn leaderboard(
    db: Arc<Mutex<Connection>>,
    guild_id: u64,
    today: NaiveDate,
    limit: usize,
) -> Result<Vec<LeaderEntry>> {
    tokio::task::spawn_blocking(move || {
        let connection = lock(&db);
        let mut statement = connection
            .prepare("SELECT user_id, day, verse_key FROM memorizations WHERE guild_id = ?1")?;
        let rows = statement.query_map(params![guild_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut by_user: HashMap<u64, (Vec<NaiveDate>, HashSet<String>)> = HashMap::new();
        for row in rows {
            let (user_id, day, verse) = row?;
            let (Ok(user_id), Ok(day)) = (
                user_id.parse::<u64>(),
                NaiveDate::parse_from_str(&day, "%Y-%m-%d"),
            ) else {
                continue;
            };
            let entry = by_user.entry(user_id).or_default();
            entry.0.push(day);
            entry.1.insert(verse);
        }

        let mut entries: Vec<LeaderEntry> = by_user
            .into_iter()
            .map(|(user_id, (days, verses))| {
                let known: HashSet<NaiveDate> = days.into_iter().collect();
                let (streak, _) = active_streak(&known, today);
                LeaderEntry {
                    user_id,
                    total: verses.len() as u32,
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const GUILD: u64 = 10;
    const OTHER_GUILD: u64 = 20;

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wird-{name}-{unique}.db"))
    }

    fn test_db(name: &str) -> (Arc<Mutex<Connection>>, PathBuf) {
        let path = temp_path(name);
        let connection = open(path.to_str().unwrap()).unwrap();
        (Arc::new(Mutex::new(connection)), path)
    }

    #[tokio::test]
    async fn a_user_advances_only_after_marking_one_ayah_per_day() {
        let (db, path) = test_db("test");
        let plan = set_plan(db.clone(), GUILD, 1, None, None).await.unwrap();

        assert_eq!(plan.next_verse, Some((1, 1)));
        assert_eq!(
            verses_for_day(db.clone(), GUILD, 1, "2026-11-12")
                .await
                .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "1:1",
                plan,
                Some((1, 2))
            )
            .await
            .unwrap(),
            MarkResult::Inserted
        );

        let next_plan = get_plan(db.clone(), GUILD, 1).await.unwrap().unwrap();
        assert_eq!(next_plan.next_verse, Some((1, 2)));
        assert_eq!(
            verses_for_day(db.clone(), GUILD, 1, "2026-11-12")
                .await
                .unwrap(),
            vec!["1:1".to_owned()]
        );
        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "1:2",
                next_plan,
                Some((1, 3))
            )
            .await
            .unwrap(),
            MarkResult::DailyLimit
        );
        assert_eq!(
            get_plan(db.clone(), GUILD, 1)
                .await
                .unwrap()
                .unwrap()
                .next_verse,
            Some((1, 2))
        );

        let stats = stats(db, GUILD, 1, NaiveDate::from_ymd_opt(2026, 11, 12).unwrap())
            .await
            .unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.streak, 1);
        assert!(stats.done_today);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_plan_can_require_multiple_ayahs_per_day() {
        let (db, path) = test_db("multi");
        let mut plan = set_plan(db.clone(), GUILD, 1, Some(1), Some(3))
            .await
            .unwrap();
        assert_eq!(plan.daily_target, 3);

        for (verse_key, next_verse) in [
            ("1:1", Some((1, 2))),
            ("1:2", Some((1, 3))),
            ("1:3", Some((1, 4))),
        ] {
            assert_eq!(
                mark_done(
                    db.clone(),
                    GUILD,
                    1,
                    "2026-11-12",
                    verse_key,
                    plan,
                    next_verse
                )
                .await
                .unwrap(),
                MarkResult::Inserted
            );
            plan = get_plan(db.clone(), GUILD, 1).await.unwrap().unwrap();
        }

        assert_eq!(
            verses_for_day(db.clone(), GUILD, 1, "2026-11-12")
                .await
                .unwrap(),
            vec!["1:1".to_owned(), "1:2".to_owned(), "1:3".to_owned()]
        );
        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "1:4",
                plan,
                Some((1, 5))
            )
            .await
            .unwrap(),
            MarkResult::DailyLimit
        );
        assert_eq!(plan.next_verse, Some((1, 4)));

        let stats = stats(db, GUILD, 1, NaiveDate::from_ymd_opt(2026, 11, 12).unwrap())
            .await
            .unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.streak, 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn each_user_keeps_an_independent_plan_and_surah_plans_stop_at_the_end() {
        let (db, path) = test_db("plans");
        let first = set_plan(db.clone(), GUILD, 1, None, None).await.unwrap();
        let second = set_plan(db.clone(), GUILD, 2, Some(2), None).await.unwrap();

        assert_eq!(first.next_verse, Some((1, 1)));
        assert_eq!(second.next_verse, Some((2, 1)));
        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "1:1",
                first,
                Some((1, 2))
            )
            .await
            .unwrap(),
            MarkResult::Inserted
        );
        assert_eq!(
            get_plan(db.clone(), GUILD, 1)
                .await
                .unwrap()
                .unwrap()
                .next_verse,
            Some((1, 2))
        );
        assert_eq!(
            get_plan(db.clone(), GUILD, 2)
                .await
                .unwrap()
                .unwrap()
                .next_verse,
            Some((2, 1))
        );
        assert_eq!(
            set_plan(db.clone(), GUILD, 1, None, None)
                .await
                .unwrap()
                .next_verse,
            Some((1, 2))
        );
        assert_eq!(
            set_plan(db.clone(), GUILD, 1, Some(3), None)
                .await
                .unwrap()
                .next_verse,
            Some((3, 1))
        );

        let mut final_ayah = set_plan(db.clone(), GUILD, 3, Some(1), None).await.unwrap();
        for (day, verse_key, next_verse) in [
            ("2026-11-06", "1:1", Some((1, 2))),
            ("2026-11-07", "1:2", Some((1, 3))),
            ("2026-11-08", "1:3", Some((1, 4))),
            ("2026-11-09", "1:4", Some((1, 5))),
            ("2026-11-10", "1:5", Some((1, 6))),
            ("2026-11-11", "1:6", Some((1, 7))),
        ] {
            assert_eq!(
                mark_done(db.clone(), GUILD, 3, day, verse_key, final_ayah, next_verse)
                    .await
                    .unwrap(),
                MarkResult::Inserted
            );
            final_ayah = get_plan(db.clone(), GUILD, 3).await.unwrap().unwrap();
        }
        let completed = Plan {
            target_surah: Some(1),
            next_verse: None,
            daily_target: 1,
        };
        assert_eq!(
            mark_done(db.clone(), GUILD, 3, "2026-11-12", "1:7", final_ayah, None)
                .await
                .unwrap(),
            MarkResult::Inserted
        );
        assert_eq!(
            get_plan(db.clone(), GUILD, 3).await.unwrap(),
            Some(completed)
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn leaderboard_ranks_by_current_streak() {
        let (db, path) = test_db("leader");
        set_plan(db.clone(), GUILD, 1, None, None).await.unwrap();
        set_plan(db.clone(), GUILD, 2, None, None).await.unwrap();
        set_plan(db.clone(), GUILD, 3, None, None).await.unwrap();

        for (day, verse, next) in [
            ("2026-11-10", "1:1", Some((1, 2))),
            ("2026-11-11", "1:2", Some((1, 3))),
            ("2026-11-12", "1:3", Some((1, 4))),
        ] {
            let plan = get_plan(db.clone(), GUILD, 1).await.unwrap().unwrap();
            assert_eq!(
                mark_done(db.clone(), GUILD, 1, day, verse, plan, next)
                    .await
                    .unwrap(),
                MarkResult::Inserted
            );
        }
        let plan = get_plan(db.clone(), GUILD, 2).await.unwrap().unwrap();
        mark_done(
            db.clone(),
            GUILD,
            2,
            "2026-11-12",
            "1:1",
            plan,
            Some((1, 2)),
        )
        .await
        .unwrap();
        let plan = get_plan(db.clone(), GUILD, 3).await.unwrap().unwrap();
        mark_done(
            db.clone(),
            GUILD,
            3,
            "2026-11-01",
            "1:1",
            plan,
            Some((1, 2)),
        )
        .await
        .unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let entries = leaderboard(db.clone(), GUILD, today, 10).await.unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].user_id, 1);
        assert_eq!(entries[0].streak, 3);
        assert_eq!(entries[0].total, 3);
        assert_eq!(entries[1].user_id, 2);
        assert_eq!(entries[1].streak, 1);
        assert_eq!(entries[2].user_id, 3);
        assert_eq!(entries[2].streak, 0);

        let top = leaderboard(db.clone(), GUILD, today, 1).await.unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].user_id, 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn each_server_keeps_its_own_plan_and_progress_for_the_same_user() {
        let (db, path) = test_db("guilds");
        let here = set_plan(db.clone(), GUILD, 1, None, None).await.unwrap();
        let there = set_plan(db.clone(), OTHER_GUILD, 1, Some(2), None)
            .await
            .unwrap();

        assert_eq!(here.next_verse, Some((1, 1)));
        assert_eq!(there.next_verse, Some((2, 1)));

        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "1:1",
                here,
                Some((1, 2))
            )
            .await
            .unwrap(),
            MarkResult::Inserted
        );

        assert_eq!(
            get_plan(db.clone(), GUILD, 1)
                .await
                .unwrap()
                .unwrap()
                .next_verse,
            Some((1, 2))
        );
        assert_eq!(
            get_plan(db.clone(), OTHER_GUILD, 1)
                .await
                .unwrap()
                .unwrap()
                .next_verse,
            Some((2, 1))
        );

        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        assert_eq!(stats(db.clone(), GUILD, 1, today).await.unwrap().total, 1);
        assert_eq!(
            stats(db.clone(), OTHER_GUILD, 1, today)
                .await
                .unwrap()
                .total,
            0
        );
        assert!(leaderboard(db.clone(), OTHER_GUILD, today, 10)
            .await
            .unwrap()
            .is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reset_clears_progress_and_restarts_the_plan_only_for_that_guild() {
        let (db, path) = test_db("reset");
        let plan = set_plan(db.clone(), GUILD, 1, Some(2), None).await.unwrap();
        let other = set_plan(db.clone(), OTHER_GUILD, 1, Some(2), None)
            .await
            .unwrap();

        assert_eq!(
            mark_done(
                db.clone(),
                GUILD,
                1,
                "2026-11-12",
                "2:1",
                plan,
                Some((2, 2))
            )
            .await
            .unwrap(),
            MarkResult::Inserted
        );
        assert_eq!(
            mark_done(
                db.clone(),
                OTHER_GUILD,
                1,
                "2026-11-12",
                "2:1",
                other,
                Some((2, 2))
            )
            .await
            .unwrap(),
            MarkResult::Inserted
        );

        reset(db.clone(), GUILD, 1).await.unwrap();

        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let after = stats(db.clone(), GUILD, 1, today).await.unwrap();
        assert_eq!(after.total, 0);
        assert_eq!(after.streak, 0);
        assert!(!after.done_today);

        let restarted = get_plan(db.clone(), GUILD, 1).await.unwrap().unwrap();
        assert_eq!(restarted.target_surah, Some(2));
        assert_eq!(restarted.next_verse, Some((2, 1)));

        assert_eq!(
            stats(db.clone(), OTHER_GUILD, 1, today)
                .await
                .unwrap()
                .total,
            1
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_new_database_starts_at_the_current_schema_version() {
        let path = temp_path("fresh");
        let connection = open(path.to_str().unwrap()).unwrap();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 2);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_guild_scoped_database_learns_the_daily_target_without_losing_progress() {
        let path = temp_path("upgrade");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE memorizations (
                         guild_id   TEXT NOT NULL,
                         user_id    TEXT NOT NULL,
                         day        TEXT NOT NULL,
                         verse_key  TEXT NOT NULL,
                         created_at TEXT NOT NULL DEFAULT (datetime('now')),
                         PRIMARY KEY (guild_id, user_id, day)
                     );
                     CREATE TABLE user_plans (
                         guild_id     TEXT NOT NULL,
                         user_id      TEXT NOT NULL,
                         target_surah INTEGER,
                         next_surah   INTEGER,
                         next_ayah    INTEGER,
                         PRIMARY KEY (guild_id, user_id),
                         CHECK ((next_surah IS NULL) = (next_ayah IS NULL))
                     );
                     INSERT INTO user_plans (guild_id, user_id, target_surah, next_surah, next_ayah)
                         VALUES ('1', '7', NULL, 1, 2);
                     INSERT INTO memorizations (guild_id, user_id, day, verse_key)
                         VALUES ('1', '7', '2026-11-12', '1:1');",
                )
                .unwrap();
        }

        let connection = open(path.to_str().unwrap()).unwrap();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let plan: (Option<u16>, Option<u16>, u16) = connection
            .query_row(
                "SELECT next_surah, next_ayah, daily_target FROM user_plans
                 WHERE guild_id = '1' AND user_id = '7'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(plan, (Some(1), Some(2), 1));

        connection
            .execute(
                "INSERT INTO memorizations (guild_id, user_id, day, verse_key)
                 VALUES ('1', '7', '2026-11-12', '1:2')",
                [],
            )
            .unwrap();
        let (day, verses): (String, u16) = connection
            .query_row(
                "SELECT day, COUNT(*) FROM memorizations
                 WHERE guild_id = '1' AND user_id = '7'
                 GROUP BY day",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((day.as_str(), verses), ("2026-11-12", 2));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_old_global_database_is_migrated_without_losing_progress() {
        let path = temp_path("migration");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE memorizations (
                         user_id    TEXT NOT NULL,
                         day        TEXT NOT NULL,
                         verse_key  TEXT NOT NULL,
                         created_at TEXT NOT NULL DEFAULT (datetime('now')),
                         PRIMARY KEY (user_id, day)
                     );
                     CREATE TABLE user_plans (
                         user_id      TEXT PRIMARY KEY NOT NULL,
                         target_surah INTEGER,
                         next_surah   INTEGER,
                         next_ayah    INTEGER,
                         CHECK ((next_surah IS NULL) = (next_ayah IS NULL))
                     );
                     INSERT INTO user_plans (user_id, target_surah, next_surah, next_ayah)
                         VALUES ('7', NULL, 1, 2);
                     INSERT INTO memorizations (user_id, day, verse_key)
                         VALUES ('7', '2026-11-12', '1:1');",
                )
                .unwrap();
        }

        let connection = open(path.to_str().unwrap()).unwrap();
        let plan: (Option<u16>, Option<u16>, Option<u16>, u16) = connection
            .query_row(
                "SELECT target_surah, next_surah, next_ayah, daily_target FROM user_plans
                 WHERE guild_id = ?1 AND user_id = '7'",
                params![LEGACY_GUILD_ID],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(plan, (None, Some(1), Some(2), 1));

        let day: String = connection
            .query_row(
                "SELECT day FROM memorizations WHERE guild_id = ?1 AND user_id = '7'",
                params![LEGACY_GUILD_ID],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(day, "2026-11-12");

        let _ = std::fs::remove_file(path);
    }
}
