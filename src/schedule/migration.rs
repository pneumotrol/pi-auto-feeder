use super::{DEFAULT_COOLDOWN_SECONDS, DEFAULT_FEED_DURATION_MS};
use color_eyre::eyre::{Result, WrapErr, bail};
use sqlx::{Executor, Row, Sqlite, Transaction};

const SCHEMA_VERSION: i64 = 1;

pub(super) async fn migrate(pool: &sqlx::SqlitePool) -> Result<()> {
    let current_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;
    if current_version > SCHEMA_VERSION {
        bail!(
            "database schema version {current_version} is newer than supported version {SCHEMA_VERSION}"
        );
    }
    let mut transaction = pool
        .begin()
        .await
        .wrap_err("failed to begin database migration")?;

    migrate_schedules(&mut transaction).await?;
    create_supporting_tables(&mut transaction).await?;
    transaction
        .execute(format!("PRAGMA user_version = {SCHEMA_VERSION}").as_str())
        .await?;
    transaction
        .commit()
        .await
        .wrap_err("failed to commit database migration")?;
    Ok(())
}

async fn migrate_schedules(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schedules'",
    )
    .fetch_one(&mut **transaction)
    .await?;

    if exists == 0 {
        create_schedules_table(transaction).await?;
        return Ok(());
    }

    let rows = sqlx::query("PRAGMA table_info(schedules)")
        .fetch_all(&mut **transaction)
        .await?;
    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        columns.push(row.try_get::<String, _>("name")?);
    }
    let has = |name: &str| columns.iter().any(|column| column == name);

    if has("time") && !has("scheduled_at") {
        transaction
            .execute("ALTER TABLE schedules RENAME TO schedules_time_only")
            .await?;
        create_schedules_table(transaction).await?;
        transaction
            .execute(
                "INSERT INTO schedules (id, scheduled_at, legacy_time)
                 SELECT id, NULL, time FROM schedules_time_only",
            )
            .await?;
        transaction
            .execute("DROP TABLE schedules_time_only")
            .await?;
        return Ok(());
    }

    if !has("scheduled_at") {
        bail!("unsupported schedules table: scheduled_at column is missing");
    }
    if !has("legacy_time") {
        transaction
            .execute("ALTER TABLE schedules ADD COLUMN legacy_time TEXT")
            .await?;
    }
    if !has("failure_reason") {
        transaction
            .execute("ALTER TABLE schedules ADD COLUMN failure_reason TEXT")
            .await?;
    }
    Ok(())
}

async fn create_schedules_table(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    transaction
        .execute(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY,
                scheduled_at TEXT UNIQUE,
                legacy_time TEXT,
                failure_reason TEXT
            )",
        )
        .await?;
    Ok(())
}

async fn create_supporting_tables(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS feeder_status (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_feed_at TEXT
            )",
        )
        .await?;
    transaction
        .execute("INSERT OR IGNORE INTO feeder_status (id) VALUES (1)")
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS feed_history (
                id INTEGER PRIMARY KEY,
                fed_at TEXT NOT NULL
            )",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cooldown_seconds INTEGER NOT NULL,
                feed_duration_ms INTEGER NOT NULL
            )",
        )
        .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO settings (id, cooldown_seconds, feed_duration_ms)
         VALUES (1, ?1, ?2)",
    )
    .bind(DEFAULT_COOLDOWN_SECONDS as i64)
    .bind(DEFAULT_FEED_DURATION_MS as i64)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
