use crate::feed::{FeedOutcome, FeedService};
use color_eyre::eyre::{Result, WrapErr};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::{env, str::FromStr, time::Duration};
use tokio::sync::broadcast;

const DEFAULT_DATABASE_URL: &str = "sqlite://pi-auto-feeder.sqlite3";
const SCHEDULER_INTERVAL: Duration = Duration::from_secs(30);
pub const DEFAULT_COOLDOWN_SECONDS: u64 = 300;
pub const DEFAULT_FEED_DURATION_MS: u64 = 1_000;
pub const MAX_COOLDOWN_SECONDS: u64 = 86_400;
pub const MIN_FEED_DURATION_MS: u64 = 100;
pub const MAX_FEED_DURATION_MS: u64 = 10_000;

#[derive(Clone)]
pub struct ScheduleStore {
    pool: SqlitePool,
    changes: broadcast::Sender<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub id: i64,
    pub scheduled_at: Option<String>,
    pub legacy_time: Option<String>,
    pub missed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub cooldown_seconds: u64,
    pub feed_duration_ms: u64,
}

pub struct ServerStatus {
    pub current_time: String,
    pub last_feed_time: Option<String>,
}

impl ScheduleStore {
    pub async fn from_env() -> Result<Self> {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        Self::connect(&database_url).await
    }

    async fn connect(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)
            .wrap_err("DATABASE_URL must be a valid SQLite URL")?
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options)
            .await
            .wrap_err("failed to open SQLite database")?;
        let (changes, _) = broadcast::channel(16);
        let store = Self { pool, changes };
        store.initialize().await?;
        Ok(store)
    }

    async fn initialize(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schedules (
                id INTEGER PRIMARY KEY,
                scheduled_at TEXT UNIQUE,
                legacy_time TEXT,
                failure_reason TEXT
            )",
        )
        .execute(&self.pool)
        .await?;
        self.migrate_time_only_schedules().await?;
        self.add_failure_reason_column().await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS feeder_status (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_feed_at TEXT
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO feeder_status (id) VALUES (1)")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS feed_history (
                id INTEGER PRIMARY KEY,
                fed_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cooldown_seconds INTEGER NOT NULL,
                feed_duration_ms INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO settings (id, cooldown_seconds, feed_duration_ms)
             VALUES (1, ?1, ?2)",
        )
        .bind(DEFAULT_COOLDOWN_SECONDS as i64)
        .bind(DEFAULT_FEED_DURATION_MS as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn add_failure_reason_column(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(schedules)")
            .fetch_all(&self.pool)
            .await?;
        if !columns
            .iter()
            .any(|column| column.get::<String, _>("name") == "failure_reason")
        {
            sqlx::query("ALTER TABLE schedules ADD COLUMN failure_reason TEXT")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn migrate_time_only_schedules(&self) -> Result<()> {
        let columns = sqlx::query("PRAGMA table_info(schedules)")
            .fetch_all(&self.pool)
            .await?;
        let has_scheduled_at = columns
            .iter()
            .any(|column| column.get::<String, _>("name") == "scheduled_at");
        let has_legacy_time = columns
            .iter()
            .any(|column| column.get::<String, _>("name") == "legacy_time");
        if has_scheduled_at && has_legacy_time {
            return Ok(());
        }
        if has_scheduled_at {
            sqlx::query("ALTER TABLE schedules ADD COLUMN legacy_time TEXT")
                .execute(&self.pool)
                .await?;
            return Ok(());
        }

        let mut transaction = self.pool.begin().await?;
        sqlx::query("ALTER TABLE schedules RENAME TO schedules_time_only")
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY,
                scheduled_at TEXT UNIQUE,
                legacy_time TEXT,
                failure_reason TEXT
            )",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO schedules (id, scheduled_at, legacy_time)
             SELECT id, NULL, time FROM schedules_time_only",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DROP TABLE schedules_time_only")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query(
            "SELECT id,
                    scheduled_at,
                    legacy_time,
                    failure_reason,
                    COALESCE(
                        scheduled_at < strftime('%Y-%m-%dT%H:%M', 'now', 'localtime'),
                        0
                    ) AS missed
             FROM schedules
             ORDER BY scheduled_at IS NOT NULL, scheduled_at",
        )
        .fetch_all(&self.pool)
        .await?;
        let schedules = rows
            .into_iter()
            .map(|row| -> sqlx::Result<Schedule> {
                Ok(Schedule {
                    id: row.try_get("id")?,
                    scheduled_at: row.try_get("scheduled_at")?,
                    legacy_time: row.try_get("legacy_time")?,
                    missed: row.try_get::<i64, _>("missed")? != 0,
                    failure_reason: row.try_get("failure_reason")?,
                })
            })
            .collect::<sqlx::Result<Vec<_>>>()?;
        Ok(schedules)
    }

    pub async fn add(&self, scheduled_at: &str) -> Result<Schedule> {
        validate_scheduled_at(scheduled_at)?;
        let row = sqlx::query(
            "INSERT INTO schedules (scheduled_at)
             SELECT ?1
             WHERE ?1 > strftime('%Y-%m-%dT%H:%M', 'now', 'localtime')
             RETURNING id, scheduled_at",
        )
        .bind(scheduled_at)
        .fetch_one(&self.pool)
        .await?;
        let schedule = Schedule {
            id: row.try_get("id")?,
            scheduled_at: row.try_get("scheduled_at")?,
            legacy_time: None,
            missed: false,
            failure_reason: None,
        };
        self.notify();
        Ok(schedule)
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    pub async fn record_feed(&self) -> Result<String> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE feeder_status
             SET last_feed_at = strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime')
             WHERE id = 1",
        )
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query(
            "INSERT INTO feed_history (fed_at)
             VALUES (strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime'))
             RETURNING fed_at",
        )
        .fetch_one(&mut *transaction)
        .await?;
        let fed_at = row.try_get("fed_at")?;
        transaction.commit().await?;
        self.notify();
        Ok(fed_at)
    }

    pub async fn recent_feed_history(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT fed_at FROM feed_history ORDER BY id DESC LIMIT 10")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.try_get("fed_at"))
            .collect::<sqlx::Result<Vec<_>>>()?)
    }

    pub async fn settings(&self) -> Result<Settings> {
        let row =
            sqlx::query("SELECT cooldown_seconds, feed_duration_ms FROM settings WHERE id = 1")
                .fetch_one(&self.pool)
                .await?;
        Ok(Settings {
            cooldown_seconds: row.try_get::<i64, _>("cooldown_seconds")? as u64,
            feed_duration_ms: row.try_get::<i64, _>("feed_duration_ms")? as u64,
        })
    }

    pub async fn update_settings(&self, settings: &Settings) -> Result<()> {
        if settings.cooldown_seconds > MAX_COOLDOWN_SECONDS
            || !(MIN_FEED_DURATION_MS..=MAX_FEED_DURATION_MS).contains(&settings.feed_duration_ms)
        {
            color_eyre::eyre::bail!("settings are outside the allowed range");
        }
        sqlx::query(
            "UPDATE settings SET cooldown_seconds = ?1, feed_duration_ms = ?2 WHERE id = 1",
        )
        .bind(settings.cooldown_seconds as i64)
        .bind(settings.feed_duration_ms as i64)
        .execute(&self.pool)
        .await?;
        self.notify();
        Ok(())
    }

    pub async fn cooldown_remaining(&self) -> Result<u64> {
        let row = sqlx::query(
            "SELECT CASE WHEN feeder_status.last_feed_at IS NULL THEN 0 ELSE MAX(
                0,
                settings.cooldown_seconds
                - (strftime('%s', 'now', 'localtime') - strftime('%s', feeder_status.last_feed_at))
             ) END AS remaining
             FROM feeder_status CROSS JOIN settings
             WHERE feeder_status.id = 1 AND settings.id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("remaining")? as u64)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    fn notify(&self) {
        let _ = self.changes.send(());
    }

    pub async fn status(&self) -> Result<ServerStatus> {
        let row = sqlx::query(
            "SELECT strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime') AS current_time,
                    last_feed_at
             FROM feeder_status
             WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ServerStatus {
            current_time: row.try_get("current_time")?,
            last_feed_time: row.try_get("last_feed_at")?,
        })
    }

    async fn due_schedule(&self) -> Result<Option<Schedule>> {
        let row = sqlx::query(
            "SELECT id, scheduled_at FROM schedules
             WHERE scheduled_at = strftime('%Y-%m-%dT%H:%M', 'now', 'localtime')
               AND failure_reason IS NULL
             ORDER BY id
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        let schedule = row
            .map(|row| -> sqlx::Result<Schedule> {
                Ok(Schedule {
                    id: row.try_get("id")?,
                    scheduled_at: row.try_get("scheduled_at")?,
                    legacy_time: None,
                    missed: false,
                    failure_reason: None,
                })
            })
            .transpose()?;
        Ok(schedule)
    }

    async fn complete_scheduled_feed(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    async fn fail_schedule(&self, id: i64, reason: &str) -> Result<()> {
        sqlx::query("UPDATE schedules SET failure_reason = ?1 WHERE id = ?2")
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }
}

pub fn start_scheduler(store: ScheduleStore, feeder: FeedService) {
    tokio::spawn(async move {
        loop {
            match store.due_schedule().await {
                Ok(Some(schedule)) => {
                    println!(
                        "Running scheduled feed at {}",
                        schedule.scheduled_at.as_deref().unwrap_or("unknown")
                    );
                    match feeder.feed().await {
                        Ok(FeedOutcome::Fed(_)) => {
                            if let Err(error) = store.complete_scheduled_feed(schedule.id).await {
                                eprintln!("Failed to consume feed schedule: {error}");
                            }
                        }
                        Ok(FeedOutcome::Cooldown(_)) => {
                            if let Err(error) = store
                                .fail_schedule(
                                    schedule.id,
                                    "クールタイム中のため給餌されませんでした",
                                )
                                .await
                            {
                                eprintln!("Failed to mark schedule as failed: {error}");
                            }
                        }
                        Err(error) => {
                            eprintln!("Scheduled feed failed: {error}");
                            if let Err(mark_error) = store
                                .fail_schedule(schedule.id, "給餌処理に失敗しました")
                                .await
                            {
                                eprintln!("Failed to mark schedule as failed: {mark_error}");
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("Failed to check feed schedules: {error}"),
            }
            tokio::time::sleep(SCHEDULER_INTERVAL).await;
        }
    });
}

fn validate_scheduled_at(value: &str) -> Result<()> {
    if !value.is_ascii()
        || value.len() != 16
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || &value[10..11] != "T"
        || &value[13..14] != ":"
    {
        color_eyre::eyre::bail!("schedule must use YYYY-MM-DDTHH:MM format");
    }
    let year = value[0..4].parse::<u16>()?;
    let month = value[5..7].parse::<u8>()?;
    let day = value[8..10].parse::<u8>()?;
    let hour = value[11..13].parse::<u8>()?;
    let minute = value[14..16].parse::<u8>()?;
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > days_in_month || hour > 23 || minute > 59 {
        color_eyre::eyre::bail!("schedule must contain a valid date and time");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_scheduled_at() {
        assert!(validate_scheduled_at("2028-02-29T23:59").is_ok());
        assert!(validate_scheduled_at("2027-02-29T12:00").is_err());
        assert!(validate_scheduled_at("2026-01-01T24:00").is_err());
        assert!(validate_scheduled_at("2026-01-01 12:00").is_err());
    }

    #[tokio::test]
    async fn migrates_time_only_schedules_without_assigning_a_date() {
        let path = format!(
            "/tmp/pi-auto-feeder-migration-test-{}.sqlite3",
            std::process::id()
        );
        let database_url = format!("sqlite://{path}");
        let options = SqliteConnectOptions::from_str(&database_url)
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY,
                time TEXT NOT NULL UNIQUE,
                last_run_date TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO schedules (id, time) VALUES (42, '07:30')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let store = ScheduleStore::connect(&database_url).await.unwrap();
        let schedules = store.list().await.unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].id, 42);
        assert_eq!(schedules[0].scheduled_at, None);
        assert_eq!(schedules[0].legacy_time.as_deref(), Some("07:30"));
        assert!(!schedules[0].missed);
        assert_eq!(store.settings().await.unwrap().cooldown_seconds, 300);
        assert_eq!(store.settings().await.unwrap().feed_duration_ms, 1_000);
        for _ in 0..11 {
            store.record_feed().await.unwrap();
        }
        assert_eq!(store.recent_feed_history().await.unwrap().len(), 10);
        assert!(
            store
                .update_settings(&Settings {
                    cooldown_seconds: MAX_COOLDOWN_SECONDS + 1,
                    feed_duration_ms: DEFAULT_FEED_DURATION_MS,
                })
                .await
                .is_err()
        );
        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }
}
