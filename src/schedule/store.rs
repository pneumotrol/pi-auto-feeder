//! SQLite を正とするスケジュール・設定・給餌履歴ストア。

use super::{
    MAX_COOLDOWN_SECONDS, MAX_FEED_DURATION_MS, MAX_FEED_SPEED_PERCENT, MIN_FEED_DURATION_MS,
    MIN_FEED_SPEED_PERCENT, Schedule, ServerStatus, Settings, migration,
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
/// SQLite 接続プールと、SSE に橋渡しするプロセス内変更通知を共有する。
pub struct ScheduleStore {
    pub(super) pool: SqlitePool,
    changes: broadcast::Sender<()>,
}

impl ScheduleStore {
    /// `DATABASE_URL`、または既定のローカル DB を開いて移行する。
    pub async fn from_env() -> Result<Self> {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        Self::connect(&database_url).await
    }

    /// 指定 URL の SQLite DB を WAL モードで開き、利用前にスキーマを移行する。
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

    /// 旧形式、期限切れ、失敗済みを含む全スケジュールを表示順で取得する。
    pub async fn list(&self) -> Result<Vec<Schedule>> {
        let rows = sqlx::query(
            "SELECT id,
                    scheduled_at,
                    failure_reason,
                    COALESCE(
                        scheduled_at < strftime('%Y-%m-%dT%H:%M', 'now', 'localtime'),
                        0
                    ) AS missed
             FROM schedules
             ORDER BY scheduled_at",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Schedule {
                    id: row.try_get("id")?,
                    scheduled_at: row.try_get("scheduled_at")?,
                    missed: row.try_get::<i64, _>("missed")? != 0,
                    failure_reason: row.try_get("failure_reason")?,
                })
            })
            .collect()
    }

    /// 書式が正しく、DB のローカル現在時刻より未来にある予定だけを追加する。
    pub async fn add(&self, scheduled_at: &str) -> Result<Schedule> {
        validate_scheduled_at(scheduled_at)?;
        // 検証と INSERT を同じ SQL 文にし、時刻が進む間の競合を避ける。
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
            missed: false,
            failure_reason: None,
        };
        self.notify();
        Ok(schedule)
    }

    /// ID が一致するスケジュールを削除する。
    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    /// 最終給餌時刻と履歴を同じトランザクションで更新する。
    pub async fn record_feed(&self) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE feeder_status
             SET last_feed_at = strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime')
             WHERE id = 1",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO feed_history (fed_at)
             VALUES (strftime('%Y-%m-%d %H:%M:%S', 'now', 'localtime'))",
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.notify();
        Ok(())
    }

    /// 新しい順に最大 10 件の給餌履歴を取得する。
    pub async fn recent_feed_history(&self) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT fed_at FROM feed_history ORDER BY id DESC LIMIT 10")
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// 保存値を符号なし整数へ変換し、許容範囲を再検証して返す。
    pub async fn settings(&self) -> Result<Settings> {
        let (cooldown_seconds, feed_duration_ms, feed_speed_percent): (i64, i64, i64) =
            sqlx::query_as(
                "SELECT cooldown_seconds, feed_duration_ms, feed_speed_percent
                 FROM settings WHERE id = 1",
            )
            .fetch_one(&self.pool)
            .await?;
        let cooldown_seconds = u64::try_from(cooldown_seconds)
            .wrap_err("stored cooldown_seconds must not be negative")?;
        let feed_duration_ms = u64::try_from(feed_duration_ms)
            .wrap_err("stored feed_duration_ms must not be negative")?;
        let feed_speed_percent = u64::try_from(feed_speed_percent)
            .wrap_err("stored feed_speed_percent must not be negative")?;
        let settings = Settings {
            cooldown_seconds,
            feed_duration_ms,
            feed_speed_percent,
        };
        validate_settings(&settings)?;
        Ok(settings)
    }

    /// 許容範囲内の給餌設定を単一行へ保存する。
    pub async fn update_settings(&self, settings: &Settings) -> Result<()> {
        validate_settings(settings)?;
        let cooldown_seconds = settings.cooldown_seconds as i64;
        let feed_duration_ms = settings.feed_duration_ms as i64;
        let feed_speed_percent = settings.feed_speed_percent as i64;
        sqlx::query(
            "UPDATE settings
             SET cooldown_seconds = ?1, feed_duration_ms = ?2, feed_speed_percent = ?3
             WHERE id = 1",
        )
        .bind(cooldown_seconds)
        .bind(feed_duration_ms)
        .bind(feed_speed_percent)
        .execute(&self.pool)
        .await?;
        self.notify();
        Ok(())
    }

    /// DB のローカル現在時刻を基準にクールタイムの残り秒数を求める。
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

    /// 永続状態が変わったことを受け取る新しい購読者を作る。
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    #[cfg(test)]
    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }

    /// サーバの現在時刻と最後の給餌成功時刻を同じ問い合わせで取得する。
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

    /// 現在の分に一致する未処理予定を一件だけ原子的に実行中へ変更する。
    pub(super) async fn claim_due_schedule(&self) -> Result<Option<Schedule>> {
        // 先に失敗理由を仮記録し、実行中のプロセスが落ちても再起動後に二重給餌しない。
        let row: Option<(i64, String)> = sqlx::query_as(
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
            missed: false,
            failure_reason: None,
        }))
    }

    /// 成功した一回限りの予定を消費して削除する。
    pub(super) async fn complete_scheduled_feed(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    /// 再実行しない予定として利用者向けの失敗理由を保存する。
    pub(super) async fn fail_schedule(&self, id: i64, reason: &str) -> Result<()> {
        sqlx::query("UPDATE schedules SET failure_reason = ?1 WHERE id = ?2")
            .bind(reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.notify();
        Ok(())
    }

    /// SSE 側は最新状態を再取得するだけなので、受信者不在や通知破棄は無視する。
    fn notify(&self) {
        let _ = self.changes.send(());
    }
}

/// `datetime-local` が送る分精度の日時書式と実在日時を検証する。
fn validate_scheduled_at(value: &str) -> Result<()> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .wrap_err("schedule must contain a valid date and time in YYYY-MM-DDTHH:MM format")?;
    Ok(())
}

/// UI の属性値に依存せず、永続化境界で設定範囲を保証する。
fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.cooldown_seconds > MAX_COOLDOWN_SECONDS
        || !(MIN_FEED_DURATION_MS..=MAX_FEED_DURATION_MS).contains(&settings.feed_duration_ms)
        || !(MIN_FEED_SPEED_PERCENT..=MAX_FEED_SPEED_PERCENT).contains(&settings.feed_speed_percent)
    {
        bail!("settings are outside the allowed range");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{
        DEFAULT_COOLDOWN_SECONDS, DEFAULT_FEED_DURATION_MS, DEFAULT_FEED_SPEED_PERCENT,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

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
                feed_speed_percent: DEFAULT_FEED_SPEED_PERCENT,
            }
        );

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
                    feed_speed_percent: DEFAULT_FEED_SPEED_PERCENT,
                })
                .await
                .is_err()
        );
        assert!(
            store
                .update_settings(&Settings {
                    cooldown_seconds: DEFAULT_COOLDOWN_SECONDS,
                    feed_duration_ms: DEFAULT_FEED_DURATION_MS,
                    feed_speed_percent: MAX_FEED_SPEED_PERCENT + 1,
                })
                .await
                .is_err()
        );
        assert!(
            store
                .update_settings(&Settings {
                    cooldown_seconds: DEFAULT_COOLDOWN_SECONDS,
                    feed_duration_ms: DEFAULT_FEED_DURATION_MS,
                    feed_speed_percent: 0,
                })
                .await
                .is_err()
        );
        let updated = Settings {
            cooldown_seconds: 60,
            feed_duration_ms: 500,
            feed_speed_percent: 50,
        };
        store.update_settings(&updated).await.unwrap();
        assert_eq!(store.settings().await.unwrap(), updated);

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
        assert_eq!(added.scheduled_at, future);
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
        assert!(!claimed.scheduled_at.is_empty());
        assert!(store.claim_due_schedule().await.unwrap().is_none());
        assert!(store.list().await.unwrap()[0].failure_reason.is_some());

        store.pool.close().await;
        tokio::fs::remove_file(path).await.unwrap();
    }
}
