use super::{
    MAX_COOLDOWN_SECONDS, MAX_FEED_DURATION_MS, MIN_FEED_DURATION_MS, Schedule, ServerStatus,
    Settings, migration,
};
use chrono::NaiveDateTime;
use color_eyre::eyre::{Result, WrapErr, bail};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use std::{env, str::FromStr, time::Duration};
use tokio::sync::broadcast;

const DEFAULT_DATABASE_URL: &str = "sqlite://pi-auto-feeder.sqlite3";

#[derive(Clone)]
pub struct ScheduleStore {
    pub(super) pool: SqlitePool,
    changes: broadcast::Sender<()>,
}

impl ScheduleStore {
    pub async fn from_env() -> Result<Self> {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        Self::connect(&database_url).await
    }

    pub(crate) async fn connect(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)
            .wrap_err("DATABASE_URL must be a valid SQLite URL")?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePool::connect_with(options)
            .await
            .wrap_err("failed to open SQLite database")?;
        migration::migrate(&pool).await?;
        let (changes, _) = broadcast::channel(16);
        Ok(Self { pool, changes })
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
        rows.into_iter()
            .map(|row| {
                Ok(Schedule {
                    id: row.try_get("id")?,
                    scheduled_at: row.try_get("scheduled_at")?,
                    legacy_time: row.try_get("legacy_time")?,
                    missed: row.try_get::<i64, _>("missed")? != 0,
                    failure_reason: row.try_get("failure_reason")?,
                })
            })
            .collect()
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
        let fed_at: String = sqlx::query_scalar(
            "INSERT INTO feed_history (fed_at)
             VALUES (strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime'))
             RETURNING fed_at",
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.notify();
        Ok(fed_at)
    }

    pub async fn recent_feed_history(&self) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT fed_at FROM feed_history ORDER BY id DESC LIMIT 10")
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn settings(&self) -> Result<Settings> {
        let (cooldown_seconds, feed_duration_ms): (i64, i64) =
            sqlx::query_as("SELECT cooldown_seconds, feed_duration_ms FROM settings WHERE id = 1")
                .fetch_one(&self.pool)
                .await?;
        let cooldown_seconds = u64::try_from(cooldown_seconds)
            .wrap_err("stored cooldown_seconds must not be negative")?;
        let feed_duration_ms = u64::try_from(feed_duration_ms)
            .wrap_err("stored feed_duration_ms must not be negative")?;
        let settings = Settings {
            cooldown_seconds,
            feed_duration_ms,
        };
        validate_settings(&settings)?;
        Ok(settings)
    }

    pub async fn update_settings(&self, settings: &Settings) -> Result<()> {
        validate_settings(settings)?;
        let cooldown_seconds = settings.cooldown_seconds as i64;
        let feed_duration_ms = settings.feed_duration_ms as i64;
        sqlx::query(
            "UPDATE settings SET cooldown_seconds = ?1, feed_duration_ms = ?2 WHERE id = 1",
        )
        .bind(cooldown_seconds)
        .bind(feed_duration_ms)
        .execute(&self.pool)
        .await?;
        self.notify();
        Ok(())
    }

    pub async fn cooldown_remaining(&self) -> Result<u64> {
        let remaining: i64 = sqlx::query_scalar(
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
        u64::try_from(remaining).wrap_err("calculated cooldown must not be negative")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    #[cfg(test)]
    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn status(&self) -> Result<ServerStatus> {
        let (current_time, last_feed_time): (String, Option<String>) = sqlx::query_as(
            "SELECT strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime') AS current_time,
                    last_feed_at
             FROM feeder_status
             WHERE id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ServerStatus {
            current_time,
            last_feed_time,
        })
    }

    pub(super) async fn claim_due_schedule(&self) -> Result<Option<Schedule>> {
        let row: Option<(i64, Option<String>)> = sqlx::query_as(
            "UPDATE schedules
             SET failure_reason = '給餌処理を開始しましたが，完了を確認できていません'
             WHERE id = (
                 SELECT id FROM schedules
                 WHERE scheduled_at = strftime('%Y-%m-%dT%H:%M', 'now', 'localtime')
                   AND failure_reason IS NULL
                 ORDER BY id
                 LIMIT 1
             )
             RETURNING id, scheduled_at",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, scheduled_at)| Schedule {
            id,
            scheduled_at,
            legacy_time: None,
            missed: false,
            failure_reason: None,
        }))
    }

    pub(super) async fn complete_scheduled_feed(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    pub(super) async fn fail_schedule(&self, id: i64, reason: &str) -> Result<()> {
        sqlx::query("UPDATE schedules SET failure_reason = ?1 WHERE id = ?2")
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    fn notify(&self) {
        let _ = self.changes.send(());
    }
}

fn validate_scheduled_at(value: &str) -> Result<()> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .wrap_err("schedule must contain a valid date and time in YYYY-MM-DDTHH:MM format")?;
    Ok(())
}

fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.cooldown_seconds > MAX_COOLDOWN_SECONDS
        || !(MIN_FEED_DURATION_MS..=MAX_FEED_DURATION_MS).contains(&settings.feed_duration_ms)
    {
        bail!("settings are outside the allowed range");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{DEFAULT_COOLDOWN_SECONDS, DEFAULT_FEED_DURATION_MS};
    use sqlx::{Executor, sqlite::SqliteConnectOptions};
    use std::{
        str::FromStr,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

    fn database_url(name: &str) -> (String, String) {
        let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let path = format!(
            "/tmp/pi-auto-feeder-{name}-{}-{id}.sqlite3",
            std::process::id()
        );
        (format!("sqlite://{path}"), path)
    }

    #[test]
    fn validates_scheduled_at() {
        assert!(validate_scheduled_at("2028-02-29T23:59").is_ok());
        assert!(validate_scheduled_at("2027-02-29T12:00").is_err());
        assert!(validate_scheduled_at("2026-01-01T24:00").is_err());
        assert!(validate_scheduled_at("2026-01-01 12:00").is_err());
    }

    #[tokio::test]
    async fn initializes_a_new_database_with_defaults() {
        let (url, path) = database_url("new");
        let store = ScheduleStore::connect(&url).await.unwrap();

        assert!(store.list().await.unwrap().is_empty());
        assert_eq!(
            store.settings().await.unwrap(),
            Settings {
                cooldown_seconds: DEFAULT_COOLDOWN_SECONDS,
                feed_duration_ms: DEFAULT_FEED_DURATION_MS,
            }
        );

        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn migrates_time_only_schedules_without_assigning_a_date() {
        let (url, path) = database_url("migration");
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        pool.execute(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY,
                time TEXT NOT NULL UNIQUE,
                last_run_date TEXT
            )",
        )
        .await
        .unwrap();
        pool.execute("INSERT INTO schedules (id, time) VALUES (42, '07:30')")
            .await
            .unwrap();
        pool.close().await;

        let store = ScheduleStore::connect(&url).await.unwrap();
        let schedules = store.list().await.unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].id, 42);
        assert_eq!(schedules[0].scheduled_at, None);
        assert_eq!(schedules[0].legacy_time.as_deref(), Some("07:30"));
        assert!(!schedules[0].missed);

        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn validates_settings_and_limits_feed_history() {
        let (url, path) = database_url("settings");
        let store = ScheduleStore::connect(&url).await.unwrap();

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

    #[tokio::test]
    async fn accepts_only_unique_future_schedules() {
        let (url, path) = database_url("schedules");
        let store = ScheduleStore::connect(&url).await.unwrap();
        let future: String =
            sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M', 'now', 'localtime', '+1 day')")
                .fetch_one(&store.pool)
                .await
                .unwrap();

        let added = store.add(&future).await.unwrap();
        assert_eq!(added.scheduled_at.as_deref(), Some(future.as_str()));
        assert!(store.add(&future).await.is_err());
        assert!(store.add("2000-01-01T00:00").await.is_err());
        assert_eq!(store.list().await.unwrap().len(), 1);

        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn due_schedule_is_claimed_at_most_once() {
        let (url, path) = database_url("claim");
        let store = ScheduleStore::connect(&url).await.unwrap();
        sqlx::query(
            "INSERT INTO schedules (scheduled_at)
             VALUES (strftime('%Y-%m-%dT%H:%M', 'now', 'localtime'))",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        let claimed = store.claim_due_schedule().await.unwrap().unwrap();
        assert!(claimed.scheduled_at.is_some());
        assert!(store.claim_due_schedule().await.unwrap().is_none());
        assert!(store.list().await.unwrap()[0].failure_reason.is_some());

        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }
}
