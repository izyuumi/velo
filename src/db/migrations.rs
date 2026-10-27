use crate::config::sqlite_path;
use crate::db::ensure_dirs;
use crate::error::{AppError, AppResult};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::time::Duration;

pub async fn init_db() -> AppResult<SqlitePool> {
    ensure_dirs()?;
    let db_path = sqlite_path();
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(30))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .min_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| AppError::Database {
            action: "opening sqlite database",
            source: e,
        })?;

    run_migrations(&pool).await?;
    super::sync_db_from_fs(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "creating schema_migrations table",
        source: e,
    })?;

    let current_version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_migrations")
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "reading schema version",
            source: e,
        })?;

    if current_version < 1 {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS videos (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('processing', 'ready', 'error')),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                error_message TEXT,
                error_detail TEXT
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "creating videos table",
            source: e,
        })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_videos_created_at ON videos(created_at DESC)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "creating videos index on created_at",
                source: e,
            })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_videos_status ON videos(status)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "creating videos index on status",
                source: e,
            })?;

        sqlx::query("INSERT INTO schema_migrations (version) VALUES (1)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "recording schema migration",
                source: e,
            })?;
    }

    if current_version < 2 {
        sqlx::query("ALTER TABLE videos ADD COLUMN duration_seconds INTEGER")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding duration_seconds column",
                source: e,
            })?;
        sqlx::query("ALTER TABLE videos ADD COLUMN width INTEGER")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding width column",
                source: e,
            })?;
        sqlx::query("ALTER TABLE videos ADD COLUMN height INTEGER")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding height column",
                source: e,
            })?;
        sqlx::query("ALTER TABLE videos ADD COLUMN progress REAL")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding progress column",
                source: e,
            })?;
        sqlx::query("ALTER TABLE videos ADD COLUMN retries INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding retries column",
                source: e,
            })?;
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (2)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "recording schema migration",
                source: e,
            })?;
    }

    if current_version < 3 {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS series (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                cover_video_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "creating series table",
            source: e,
        })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_series_created_at ON series(created_at DESC)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "creating series index on created_at",
                source: e,
            })?;

        sqlx::query("ALTER TABLE videos ADD COLUMN series_id TEXT REFERENCES series(id)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding series_id column",
                source: e,
            })?;

        sqlx::query("ALTER TABLE videos ADD COLUMN episode_number INTEGER")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "adding episode_number column",
                source: e,
            })?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_videos_series ON videos(series_id, episode_number)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "creating videos index on series_id",
                source: e,
            })?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS watch_progress (
                video_id TEXT PRIMARY KEY REFERENCES videos(id) ON DELETE CASCADE,
                position_seconds INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "creating watch_progress table",
            source: e,
        })?;

        sqlx::query("INSERT INTO schema_migrations (version) VALUES (3)")
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "recording schema migration",
                source: e,
            })?;
    }

    Ok(())
}
