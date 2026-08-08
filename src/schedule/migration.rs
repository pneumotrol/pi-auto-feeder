//! SQLite スキーマを初期化し、対応するバージョンだけを開く。

use super::{DEFAULT_COOLDOWN_SECONDS, DEFAULT_FEED_DURATION_MS};
use color_eyre::eyre::{Result, WrapErr, bail};
use sqlx::{Executor, Sqlite, Transaction};

const SCHEMA_VERSION: i64 = 1;

/// 新規 DB に現行スキーマを作成し、対応外の DB は変更せず拒否する。
pub(super) async fn migrate(pool: &sqlx::SqlitePool) -> Result<()> {
    let current_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;
    if current_version > SCHEMA_VERSION {
        bail!(
            "database schema version {current_version} is newer than supported version {SCHEMA_VERSION}"
        );
    }
    if current_version == SCHEMA_VERSION {
        return Ok(());
    }

    let mut transaction = pool
        .begin()
        .await
        .wrap_err("failed to begin database initialization")?;
    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&mut *transaction)
    .await?;
    if table_count > 0 {
        bail!("unversioned existing database is not supported");
    }
    create_schema(&mut transaction).await?;
    transaction
        .execute(format!("PRAGMA user_version = {SCHEMA_VERSION}").as_str())
        .await?;
    transaction
        .commit()
        .await
        .wrap_err("failed to commit database initialization")?;
    Ok(())
}

async fn create_schema(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    transaction
        .execute(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY,
                scheduled_at TEXT NOT NULL UNIQUE,
                failure_reason TEXT
            )",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE feeder_status (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_feed_at TEXT
            )",
        )
        .await?;
    transaction
        .execute("INSERT INTO feeder_status (id) VALUES (1)")
        .await?;
    transaction
        .execute(
            "CREATE TABLE feed_history (
                id INTEGER PRIMARY KEY,
                fed_at TEXT NOT NULL
            )",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cooldown_seconds INTEGER NOT NULL,
                feed_duration_ms INTEGER NOT NULL
            )",
        )
        .await?;
    sqlx::query(
        "INSERT INTO settings (id, cooldown_seconds, feed_duration_ms)
         VALUES (1, ?1, ?2)",
    )
    .bind(DEFAULT_COOLDOWN_SECONDS as i64)
    .bind(DEFAULT_FEED_DURATION_MS as i64)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn rejects_an_unversioned_existing_schema() {
        let pool = pool().await;
        sqlx::query("CREATE TABLE existing_data (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();

        assert!(migrate(&pool).await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA user_version")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn rejects_a_newer_schema_version() {
        let pool = pool().await;
        sqlx::query("PRAGMA user_version = 2")
            .execute(&pool)
            .await
            .unwrap();

        assert!(migrate(&pool).await.is_err());
    }
}
