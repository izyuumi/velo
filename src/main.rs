#[macro_use]
extern crate rocket;

mod error;

use rocket::form::Form;
use rocket::fairing::AdHoc;
use rocket::fs::{FileServer, NamedFile, TempFile};
use rocket::http::{ContentType, Status};
use rocket::Request;
use rocket::serde::json::Json;
use rocket::tokio::process::Command;
use rocket::tokio::task;
use rocket::State;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::{env, fs};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::time::{sleep, Instant};
use uuid::Uuid;
use crate::error::{AppError, AppResult, ApiError, to_api_error};

const STATUS_PROCESSING: &str = "processing";
const STATUS_READY: &str = "ready";
const STATUS_ERROR: &str = "error";
const MAX_LIST_VIDEOS: i64 = 200;
const PROCESSING_LOCK_STALE_SECS: u64 = 6 * 60 * 60;
const UPLOAD_SCAN_INTERVAL_SECS: u64 = 10;
const PROCESSING_DIR_SUFFIX: &str = ".processing";
const DEFAULT_MAX_RETRIES: i64 = 2;
const DEFAULT_RETRY_DELAY_SECS: u64 = 30;
const PROGRESS_UPDATE_INTERVAL_MS: u64 = 500;

struct AppState {
    db: SqlitePool,
    hls: HlsSettings,
    retry: RetrySettings,
}

#[derive(Clone)]
struct HlsVariant {
    width: i64,
    bitrate_k: i64,
}

#[derive(Clone)]
struct HlsSettings {
    segment_seconds: u32,
    crf: u8,
    preset: String,
    audio_bitrate_k: i64,
    variants: Vec<HlsVariant>,
}

#[derive(Clone, Copy)]
struct RetrySettings {
    max_retries: i64,
    delay_secs: u64,
}

fn data_root() -> PathBuf {
    env::var("DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"))
}

fn upload_dir() -> PathBuf {
    data_root().join("uploads")
}

fn hls_dir() -> PathBuf {
    data_root().join("hls")
}

fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

fn locks_dir() -> PathBuf {
    data_root().join("locks")
}

fn final_hls_dir(id: &str) -> PathBuf {
    hls_dir().join(id)
}

fn processing_hls_dir(id: &str) -> PathBuf {
    hls_dir().join(format!("{id}{PROCESSING_DIR_SUFFIX}"))
}

fn sqlite_path() -> PathBuf {
    env::var("SQLITE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_root().join("app.db"))
}

fn hls_settings() -> HlsSettings {
    let segment_seconds = env::var("HLS_SEGMENT_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(4);
    let crf = env::var("HLS_CRF")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(23);
    let preset = env::var("HLS_PRESET").unwrap_or_else(|_| "veryfast".to_string());
    let audio_bitrate_k = env::var("HLS_AUDIO_BITRATE_K")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(128);
    let variants = env::var("HLS_VARIANTS")
        .ok()
        .map(|v| parse_variants(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            vec![
                HlsVariant { width: 1920, bitrate_k: 5000 },
                HlsVariant { width: 1280, bitrate_k: 2800 },
                HlsVariant { width: 854, bitrate_k: 1400 },
            ]
        });

    HlsSettings {
        segment_seconds,
        crf,
        preset,
        audio_bitrate_k,
        variants,
    }
}

fn parse_variants(value: &str) -> Vec<HlsVariant> {
    value
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.split(':');
            let width = parts.next()?.trim().parse::<i64>().ok()?;
            let bitrate_k = parts.next()?.trim().parse::<i64>().ok()?;
            Some(HlsVariant { width, bitrate_k })
        })
        .collect()
}

fn retry_settings() -> RetrySettings {
    let max_retries = env::var("PROCESSING_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_MAX_RETRIES);
    let delay_secs = env::var("PROCESSING_RETRY_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_RETRY_DELAY_SECS);
    RetrySettings {
        max_retries,
        delay_secs,
    }
}

fn system_time_to_unix(ts: SystemTime) -> i64 {
    ts.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn unix_ts() -> i64 {
    system_time_to_unix(SystemTime::now())
}

#[derive(FromForm)]
struct UploadForm<'r> {
    video: TempFile<'r>,
    title: Option<&'r str>,
}

#[derive(Serialize, Clone)]
struct VideoItem {
    id: String,
    title: String,
    status: String,
    progress: Option<f64>,
    error: Option<String>,
    thumbnail: Option<String>,
    duration_seconds: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    created_at: i64,
    series_id: Option<String>,
    episode_number: Option<i64>,
    watch_position: Option<i64>,
    watch_completed: bool,
}


#[derive(Serialize)]
struct UploadResult {
    id: String,
    title: String,
    watch_url: String,
}

#[derive(Serialize)]
struct StatusResponse {
    id: String,
    status: String,
    progress: Option<f64>,
    error: Option<String>,
    title: String,
    watch_url: String,
    duration_seconds: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    created_at: i64,
    retries: i64,
    upload_available: bool,
    series_id: Option<String>,
    series_title: Option<String>,
    episode_number: Option<i64>,
    next_episode_id: Option<String>,
    prev_episode_id: Option<String>,
    watch_position: Option<i64>,
    watch_completed: bool,
}

#[derive(Deserialize)]
struct RenameRequest {
    title: String,
}

#[derive(Serialize, Clone)]
struct SeriesItem {
    id: String,
    title: String,
    description: Option<String>,
    cover_video_id: Option<String>,
    cover_thumbnail: Option<String>,
    episode_count: i64,
    total_duration: Option<i64>,
    created_at: i64,
    updated_at: i64,
    watched_count: i64,
    in_progress_count: i64,
}

#[derive(Serialize)]
struct SeriesDetail {
    id: String,
    title: String,
    description: Option<String>,
    cover_video_id: Option<String>,
    episodes: Vec<VideoItem>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize)]
struct CreateSeriesRequest {
    title: String,
    description: Option<String>,
    video_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UpdateSeriesRequest {
    title: Option<String>,
    description: Option<String>,
    cover_video_id: Option<String>,
}

#[derive(Deserialize)]
struct AddVideosToSeriesRequest {
    video_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ReorderEpisodesRequest {
    video_ids: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateProgressRequest {
    position_seconds: i64,
    completed: Option<bool>,
}

#[derive(Serialize)]
struct WatchProgressResponse {
    video_id: String,
    position_seconds: i64,
    completed: bool,
}

#[derive(Deserialize)]
struct BulkDeleteRequest {
    video_ids: Vec<String>,
}

#[derive(Deserialize)]
struct BulkAddToSeriesRequest {
    video_ids: Vec<String>,
    series_id: String,
}

fn ensure_dirs() -> AppResult<()> {
    fs::create_dir_all(data_root()).map_err(|e| AppError::Storage {
        action: "creating data directory",
        source: e,
    })?;
    fs::create_dir_all(upload_dir()).map_err(|e| AppError::Storage {
        action: "creating upload directory",
        source: e,
    })?;
    fs::create_dir_all(hls_dir()).map_err(|e| AppError::Storage {
        action: "creating HLS directory",
        source: e,
    })?;
    fs::create_dir_all(Path::new("static").join("vendor")).map_err(|e| AppError::Storage {
        action: "creating static vendor directory",
        source: e,
    })?;
    fs::create_dir_all(logs_dir()).map_err(|e| AppError::Storage {
        action: "creating logs directory",
        source: e,
    })?;
    fs::create_dir_all(locks_dir()).map_err(|e| AppError::Storage {
        action: "creating locks directory",
        source: e,
    })?;
    Ok(())
}

fn list_videos_from_fs() -> Vec<VideoItem> {
    let mut items = Vec::new();
    let hls_root = hls_dir();
    let Ok(entries) = fs::read_dir(&hls_root) else {
        return items;
    };

    for entry in entries.flatten() {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }

        let Some(id) = dir_path.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        if id.ends_with(PROCESSING_DIR_SUFFIX) {
            continue;
        }

        let title = id.clone();

        let master_exists = dir_path.join("master.m3u8").exists();
        if master_exists {
            let created_at = fs::metadata(&dir_path)
                .ok()
                .and_then(|meta| meta.modified().ok())
                .map(system_time_to_unix)
                .unwrap_or_else(unix_ts);
            let thumb_url = dir_path
                .join("thumb.jpg")
                .exists()
                .then(|| format!("/hls/{id}/thumb.jpg"));

            items.push(VideoItem {
                id,
                title,
                status: STATUS_READY.to_string(),
                progress: Some(100.0),
                error: None,
                thumbnail: thumb_url,
                duration_seconds: None,
                width: None,
                height: None,
                created_at,
                series_id: None,
                episode_number: None,
                watch_position: None,
                watch_completed: false,
            });
        }
    }

    items.sort_by(|a, b| b.id.cmp(&a.id));
    items
}

async fn init_db() -> AppResult<SqlitePool> {
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
    sync_db_from_fs(&pool).await?;
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

async fn upsert_ready_video(pool: &SqlitePool, id: &str, title: &str, created_at: i64) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "INSERT INTO videos (id, title, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE
         SET status=excluded.status,
             updated_at=excluded.updated_at,
             progress=100,
             created_at=MIN(videos.created_at, excluded.created_at)",
    )
    .bind(id)
    .bind(title)
    .bind(STATUS_READY)
    .bind(created_at)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "upserting ready video record",
        source: e,
    })?;

    Ok(())
}

async fn sync_db_from_fs(pool: &SqlitePool) -> AppResult<()> {
    let hls_root = hls_dir();
    let Ok(entries) = fs::read_dir(&hls_root) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }

        let Some(id) = dir_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if id.ends_with(PROCESSING_DIR_SUFFIX) {
            continue;
        }

        let master_exists = dir_path.join("master.m3u8").exists();
        if !master_exists {
            continue;
        }

        let title = id.to_string();

        let created_at = fs::metadata(&dir_path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(system_time_to_unix)
            .unwrap_or_else(unix_ts);

        upsert_ready_video(pool, id, &title, created_at).await?;
    }

    Ok(())
}

async fn insert_video(pool: &SqlitePool, id: &str, title: &str) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "INSERT INTO videos (id, title, status, created_at, updated_at, progress)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE
         SET status=excluded.status,
             updated_at=excluded.updated_at,
             error_message=NULL,
             error_detail=NULL,
             progress=excluded.progress",
    )
    .bind(id)
    .bind(title)
    .bind(STATUS_PROCESSING)
    .bind(now)
    .bind(now)
    .bind(0.0_f64)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "inserting video record",
        source: e,
    })?;

    Ok(())
}

async fn mark_video_ready(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET status = ?,
             updated_at = ?,
             progress = 100,
             error_message = NULL,
             error_detail = NULL
         WHERE id = ?",
    )
    .bind(STATUS_READY)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "updating video status to ready",
        source: e,
    })?;

    Ok(())
}

async fn mark_video_error(pool: &SqlitePool, id: &str, message: &str, detail: &str) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET status = ?,
             updated_at = ?,
             error_message = ?,
             error_detail = ?
         WHERE id = ?",
    )
    .bind(STATUS_ERROR)
    .bind(now)
    .bind(message)
    .bind(detail)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "updating video status to error",
        source: e,
    })?;

    Ok(())
}

async fn update_video_progress(pool: &SqlitePool, id: &str, progress: f64) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET progress = ?,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(progress)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "updating video progress",
        source: e,
    })?;
    Ok(())
}

async fn update_video_metadata(
    pool: &SqlitePool,
    id: &str,
    duration_seconds: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET duration_seconds = COALESCE(?, duration_seconds),
             width = COALESCE(?, width),
             height = COALESCE(?, height),
             updated_at = ?
         WHERE id = ?",
    )
    .bind(duration_seconds)
    .bind(width)
    .bind(height)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "updating video metadata",
        source: e,
    })?;
    Ok(())
}

async fn update_video_title(pool: &SqlitePool, id: &str, title: &str) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET title = ?,
             updated_at = ?,
             error_message = NULL
         WHERE id = ?",
    )
    .bind(title)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "renaming video",
        source: e,
    })?;
    Ok(())
}

async fn bump_retry_count(pool: &SqlitePool, id: &str) -> AppResult<i64> {
    sqlx::query("UPDATE videos SET retries = retries + 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "incrementing retry count",
            source: e,
        })?;

    let retries: i64 = sqlx::query_scalar("SELECT retries FROM videos WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "reading retry count",
            source: e,
        })?;
    Ok(retries)
}

async fn reset_for_reprocess(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "UPDATE videos
         SET status = ?,
             progress = 0,
             updated_at = ?,
             error_message = NULL,
             error_detail = NULL
         WHERE id = ?",
    )
    .bind(STATUS_PROCESSING)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "resetting video for reprocess",
        source: e,
    })?;
    Ok(())
}

async fn list_videos(pool: &SqlitePool, search: Option<&str>) -> AppResult<Vec<VideoItem>> {
    let search_pattern = search.map(|s| format!("%{s}%"));
    let rows = if search_pattern.is_some() {
        sqlx::query(
            "SELECT v.id, v.title, v.status, v.error_message, v.progress, v.duration_seconds,
                    v.width, v.height, v.created_at, v.series_id, v.episode_number,
                    wp.position_seconds as watch_position, wp.completed as watch_completed
             FROM videos v
             LEFT JOIN watch_progress wp ON v.id = wp.video_id
             WHERE v.title LIKE ?
             ORDER BY v.created_at DESC
             LIMIT ?",
        )
        .bind(&search_pattern)
        .bind(MAX_LIST_VIDEOS)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            "SELECT v.id, v.title, v.status, v.error_message, v.progress, v.duration_seconds,
                    v.width, v.height, v.created_at, v.series_id, v.episode_number,
                    wp.position_seconds as watch_position, wp.completed as watch_completed
             FROM videos v
             LEFT JOIN watch_progress wp ON v.id = wp.video_id
             ORDER BY v.created_at DESC
             LIMIT ?",
        )
        .bind(MAX_LIST_VIDEOS)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| AppError::Database {
        action: "listing videos",
        source: e,
    })?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        if id.is_empty() {
            continue;
        }

        let title: String = row.try_get("title").unwrap_or_else(|_| id.clone());
        let status: String = row.try_get("status").unwrap_or_else(|_| STATUS_PROCESSING.to_string());
        let error: Option<String> = row.try_get("error_message").ok();
        let progress: Option<f64> = row.try_get("progress").ok();
        let duration_seconds: Option<i64> = row.try_get("duration_seconds").ok();
        let width: Option<i64> = row.try_get("width").ok();
        let height: Option<i64> = row.try_get("height").ok();
        let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
        let series_id: Option<String> = row.try_get("series_id").ok().flatten();
        let episode_number: Option<i64> = row.try_get("episode_number").ok().flatten();
        let watch_position: Option<i64> = row.try_get("watch_position").ok().flatten();
        let watch_completed: bool = row.try_get::<i64, _>("watch_completed").ok().map(|v| v != 0).unwrap_or(false);
        let display_title = if title.trim().is_empty() { id.clone() } else { title };

        let mut status = status;
        let mut error = error;
        let mut thumb_url = None;

        if status == STATUS_READY {
            let dir_path = final_hls_dir(&id);
            let master_exists = dir_path.join("master.m3u8").exists();
            if !master_exists {
                let _ = mark_video_error(
                    pool,
                    &id,
                    "Video package missing on disk.",
                    "HLS output is missing master.m3u8.",
                )
                .await;
                status = STATUS_ERROR.to_string();
                error = Some("Video package missing on disk.".to_string());
            } else {
                thumb_url = dir_path
                    .join("thumb.jpg")
                    .exists()
                    .then(|| format!("/hls/{id}/thumb.jpg"));
            }
        }

        items.push(VideoItem {
            id,
            title: display_title,
            status,
            progress,
            error,
            thumbnail: thumb_url,
            duration_seconds,
            width,
            height,
            created_at,
            series_id,
            episode_number,
            watch_position,
            watch_completed,
        });
    }

    Ok(items)
}

async fn read_video_status_full(pool: &SqlitePool, id: &str) -> AppResult<Option<StatusResponse>> {
    let row = sqlx::query(
        "SELECT v.id, v.title, v.status, v.error_message, v.progress, v.duration_seconds,
                v.width, v.height, v.created_at, v.retries, v.series_id, v.episode_number,
                s.title as series_title,
                wp.position_seconds as watch_position, wp.completed as watch_completed
         FROM videos v
         LEFT JOIN series s ON v.series_id = s.id
         LEFT JOIN watch_progress wp ON v.id = wp.video_id
         WHERE v.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "reading full video status",
        source: e,
    })?;

    let Some(row) = row else {
        return Ok(None);
    };

    let id: String = row.try_get("id").unwrap_or_default();
    let title: String = row.try_get("title").unwrap_or_else(|_| id.clone());
    let status: String = row.try_get("status").unwrap_or_else(|_| STATUS_PROCESSING.to_string());
    let error: Option<String> = row.try_get("error_message").ok();
    let progress: Option<f64> = row.try_get("progress").ok();
    let duration_seconds: Option<i64> = row.try_get("duration_seconds").ok();
    let width: Option<i64> = row.try_get("width").ok();
    let height: Option<i64> = row.try_get("height").ok();
    let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
    let retries: i64 = row.try_get("retries").unwrap_or_default();
    let series_id: Option<String> = row.try_get("series_id").ok().flatten();
    let series_title: Option<String> = row.try_get("series_title").ok().flatten();
    let episode_number: Option<i64> = row.try_get("episode_number").ok().flatten();
    let watch_position: Option<i64> = row.try_get("watch_position").ok().flatten();
    let watch_completed: bool = row.try_get::<i64, _>("watch_completed").ok().map(|v| v != 0).unwrap_or(false);

    let (next_episode_id, prev_episode_id) = if let (Some(ref sid), Some(ep_num)) = (&series_id, episode_number) {
        let next: Option<String> = sqlx::query_scalar(
            "SELECT id FROM videos WHERE series_id = ? AND episode_number = ? AND status = 'ready'"
        )
        .bind(sid)
        .bind(ep_num + 1)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        let prev: Option<String> = if ep_num > 1 {
            sqlx::query_scalar(
                "SELECT id FROM videos WHERE series_id = ? AND episode_number = ? AND status = 'ready'"
            )
            .bind(sid)
            .bind(ep_num - 1)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        (next, prev)
    } else {
        (None, None)
    };

    let upload_available = upload_dir().join(format!("{id}.mp4")).exists();
    Ok(Some(StatusResponse {
        id: id.clone(),
        status,
        progress,
        error,
        title,
        watch_url: format!("/watch/{id}"),
        duration_seconds,
        width,
        height,
        created_at,
        retries,
        upload_available,
        series_id,
        series_title,
        episode_number,
        next_episode_id,
        prev_episode_id,
        watch_position,
        watch_completed,
    }))
}

async fn list_processing_ids(pool: &SqlitePool) -> AppResult<Vec<String>> {
    let rows = sqlx::query("SELECT id FROM videos WHERE status = ? ORDER BY created_at ASC")
        .bind(STATUS_PROCESSING)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "listing processing videos",
            source: e,
        })?;

    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        if !id.is_empty() {
            ids.push(id);
        }
    }

    Ok(ids)
}

async fn delete_video_record(pool: &SqlitePool, id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM watch_progress WHERE video_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM videos WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "deleting video record",
            source: e,
        })?;
    Ok(())
}

async fn list_series(pool: &SqlitePool) -> AppResult<Vec<SeriesItem>> {
    let rows = sqlx::query(
        "SELECT s.id, s.title, s.description, s.cover_video_id, s.created_at, s.updated_at,
                COUNT(v.id) as episode_count,
                SUM(CASE WHEN v.duration_seconds IS NOT NULL THEN v.duration_seconds ELSE 0 END) as total_duration,
                SUM(CASE WHEN wp.completed = 1 THEN 1 ELSE 0 END) as watched_count,
                SUM(CASE WHEN wp.position_seconds > 0 AND (wp.completed IS NULL OR wp.completed = 0) THEN 1 ELSE 0 END) as in_progress_count
         FROM series s
         LEFT JOIN videos v ON v.series_id = s.id AND v.status = 'ready'
         LEFT JOIN watch_progress wp ON v.id = wp.video_id
         GROUP BY s.id
         ORDER BY s.updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "listing series",
        source: e,
    })?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let title: String = row.try_get("title").unwrap_or_default();
        let description: Option<String> = row.try_get("description").ok().flatten();
        let cover_video_id: Option<String> = row.try_get("cover_video_id").ok().flatten();
        let episode_count: i64 = row.try_get("episode_count").unwrap_or(0);
        let total_duration: Option<i64> = row.try_get("total_duration").ok().flatten();
        let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
        let updated_at: i64 = row.try_get("updated_at").unwrap_or_else(|_| unix_ts());
        let watched_count: i64 = row.try_get("watched_count").unwrap_or(0);
        let in_progress_count: i64 = row.try_get("in_progress_count").unwrap_or(0);

        let cover_thumbnail = cover_video_id
            .as_ref()
            .and_then(|vid| {
                let path = final_hls_dir(vid).join("thumb.jpg");
                path.exists().then(|| format!("/hls/{vid}/thumb.jpg"))
            });

        items.push(SeriesItem {
            id,
            title,
            description,
            cover_video_id,
            cover_thumbnail,
            episode_count,
            total_duration,
            created_at,
            updated_at,
            watched_count,
            in_progress_count,
        });
    }

    Ok(items)
}

async fn get_series_detail(pool: &SqlitePool, id: &str) -> AppResult<Option<SeriesDetail>> {
    let row = sqlx::query(
        "SELECT id, title, description, cover_video_id, created_at, updated_at
         FROM series WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "getting series detail",
        source: e,
    })?;

    let Some(row) = row else {
        return Ok(None);
    };

    let series_id: String = row.try_get("id").unwrap_or_default();
    let title: String = row.try_get("title").unwrap_or_default();
    let description: Option<String> = row.try_get("description").ok().flatten();
    let cover_video_id: Option<String> = row.try_get("cover_video_id").ok().flatten();
    let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
    let updated_at: i64 = row.try_get("updated_at").unwrap_or_else(|_| unix_ts());

    let episodes = get_series_episodes(pool, &series_id).await?;

    Ok(Some(SeriesDetail {
        id: series_id,
        title,
        description,
        cover_video_id,
        episodes,
        created_at,
        updated_at,
    }))
}

async fn get_series_episodes(pool: &SqlitePool, series_id: &str) -> AppResult<Vec<VideoItem>> {
    let rows = sqlx::query(
        "SELECT v.id, v.title, v.status, v.error_message, v.progress, v.duration_seconds,
                v.width, v.height, v.created_at, v.series_id, v.episode_number,
                wp.position_seconds as watch_position, wp.completed as watch_completed
         FROM videos v
         LEFT JOIN watch_progress wp ON v.id = wp.video_id
         WHERE v.series_id = ?
         ORDER BY v.episode_number ASC",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "getting series episodes",
        source: e,
    })?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let title: String = row.try_get("title").unwrap_or_else(|_| id.clone());
        let status: String = row.try_get("status").unwrap_or_else(|_| STATUS_PROCESSING.to_string());
        let error: Option<String> = row.try_get("error_message").ok();
        let progress: Option<f64> = row.try_get("progress").ok();
        let duration_seconds: Option<i64> = row.try_get("duration_seconds").ok();
        let width: Option<i64> = row.try_get("width").ok();
        let height: Option<i64> = row.try_get("height").ok();
        let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
        let series_id: Option<String> = row.try_get("series_id").ok().flatten();
        let episode_number: Option<i64> = row.try_get("episode_number").ok().flatten();
        let watch_position: Option<i64> = row.try_get("watch_position").ok().flatten();
        let watch_completed: bool = row.try_get::<i64, _>("watch_completed").ok().map(|v| v != 0).unwrap_or(false);
        let display_title = if title.trim().is_empty() { id.clone() } else { title };

        let thumb_url = if status == STATUS_READY {
            let dir_path = final_hls_dir(&id);
            dir_path.join("thumb.jpg").exists().then(|| format!("/hls/{id}/thumb.jpg"))
        } else {
            None
        };

        items.push(VideoItem {
            id,
            title: display_title,
            status,
            progress,
            error,
            thumbnail: thumb_url,
            duration_seconds,
            width,
            height,
            created_at,
            series_id,
            episode_number,
            watch_position,
            watch_completed,
        });
    }

    Ok(items)
}

async fn create_series(pool: &SqlitePool, title: &str, description: Option<&str>) -> AppResult<String> {
    let id = Uuid::new_v4().to_string();
    let now = unix_ts();

    sqlx::query(
        "INSERT INTO series (id, title, description, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(title)
    .bind(description)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "creating series",
        source: e,
    })?;

    Ok(id)
}

async fn update_series_info(
    pool: &SqlitePool,
    id: &str,
    title: Option<&str>,
    description: Option<&str>,
    cover_video_id: Option<&str>,
) -> AppResult<()> {
    let now = unix_ts();

    if let Some(t) = title {
        sqlx::query("UPDATE series SET title = ?, updated_at = ? WHERE id = ?")
            .bind(t)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "updating series title",
                source: e,
            })?;
    }

    if let Some(d) = description {
        sqlx::query("UPDATE series SET description = ?, updated_at = ? WHERE id = ?")
            .bind(d)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "updating series description",
                source: e,
            })?;
    }

    if let Some(vid) = cover_video_id {
        sqlx::query("UPDATE series SET cover_video_id = ?, updated_at = ? WHERE id = ?")
            .bind(vid)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| AppError::Database {
                action: "updating series cover",
                source: e,
            })?;
    }

    Ok(())
}

async fn delete_series(pool: &SqlitePool, id: &str) -> AppResult<()> {
    sqlx::query("UPDATE videos SET series_id = NULL, episode_number = NULL WHERE series_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "removing videos from series",
            source: e,
        })?;

    sqlx::query("DELETE FROM series WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "deleting series",
            source: e,
        })?;

    Ok(())
}

async fn add_videos_to_series(pool: &SqlitePool, series_id: &str, video_ids: &[String]) -> AppResult<()> {
    let now = unix_ts();

    let max_ep: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(episode_number), 0) FROM videos WHERE series_id = ?",
    )
    .bind(series_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    for (idx, vid) in video_ids.iter().enumerate() {
        let ep_num = max_ep + (idx as i64) + 1;
        sqlx::query(
            "UPDATE videos SET series_id = ?, episode_number = ? WHERE id = ?",
        )
        .bind(series_id)
        .bind(ep_num)
        .bind(vid)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "adding video to series",
            source: e,
        })?;
    }

    sqlx::query("UPDATE series SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(series_id)
        .execute(pool)
        .await
        .ok();

    Ok(())
}

async fn remove_video_from_series(pool: &SqlitePool, video_id: &str) -> AppResult<()> {
    sqlx::query("UPDATE videos SET series_id = NULL, episode_number = NULL WHERE id = ?")
        .bind(video_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "removing video from series",
            source: e,
        })?;

    Ok(())
}

async fn reorder_series_episodes(pool: &SqlitePool, series_id: &str, video_ids: &[String]) -> AppResult<()> {
    let now = unix_ts();

    for (idx, vid) in video_ids.iter().enumerate() {
        let ep_num = (idx as i64) + 1;
        sqlx::query(
            "UPDATE videos SET episode_number = ? WHERE id = ? AND series_id = ?",
        )
        .bind(ep_num)
        .bind(vid)
        .bind(series_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "reordering episodes",
            source: e,
        })?;
    }

    sqlx::query("UPDATE series SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(series_id)
        .execute(pool)
        .await
        .ok();

    Ok(())
}

async fn update_watch_progress(pool: &SqlitePool, video_id: &str, position: i64, completed: bool) -> AppResult<()> {
    let now = unix_ts();

    sqlx::query(
        "INSERT INTO watch_progress (video_id, position_seconds, completed, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(video_id) DO UPDATE
         SET position_seconds = excluded.position_seconds,
             completed = excluded.completed,
             updated_at = excluded.updated_at",
    )
    .bind(video_id)
    .bind(position)
    .bind(if completed { 1 } else { 0 })
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "updating watch progress",
        source: e,
    })?;

    Ok(())
}

async fn get_watch_progress(pool: &SqlitePool, video_id: &str) -> AppResult<Option<WatchProgressResponse>> {
    let row = sqlx::query(
        "SELECT video_id, position_seconds, completed FROM watch_progress WHERE video_id = ?",
    )
    .bind(video_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "getting watch progress",
        source: e,
    })?;

    Ok(row.map(|r| WatchProgressResponse {
        video_id: r.try_get("video_id").unwrap_or_default(),
        position_seconds: r.try_get("position_seconds").unwrap_or(0),
        completed: r.try_get::<i64, _>("completed").ok().map(|v| v != 0).unwrap_or(false),
    }))
}

async fn get_continue_watching(pool: &SqlitePool) -> AppResult<Vec<VideoItem>> {
    let rows = sqlx::query(
        "SELECT v.id, v.title, v.status, v.error_message, v.progress, v.duration_seconds,
                v.width, v.height, v.created_at, v.series_id, v.episode_number,
                wp.position_seconds as watch_position, wp.completed as watch_completed
         FROM videos v
         INNER JOIN watch_progress wp ON v.id = wp.video_id
         WHERE v.status = 'ready' AND wp.completed = 0 AND wp.position_seconds > 0
         ORDER BY wp.updated_at DESC
         LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Database {
        action: "getting continue watching",
        source: e,
    })?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let title: String = row.try_get("title").unwrap_or_else(|_| id.clone());
        let status: String = row.try_get("status").unwrap_or_else(|_| STATUS_READY.to_string());
        let duration_seconds: Option<i64> = row.try_get("duration_seconds").ok();
        let width: Option<i64> = row.try_get("width").ok();
        let height: Option<i64> = row.try_get("height").ok();
        let created_at: i64 = row.try_get("created_at").unwrap_or_else(|_| unix_ts());
        let series_id: Option<String> = row.try_get("series_id").ok().flatten();
        let episode_number: Option<i64> = row.try_get("episode_number").ok().flatten();
        let watch_position: Option<i64> = row.try_get("watch_position").ok().flatten();
        let watch_completed: bool = row.try_get::<i64, _>("watch_completed").ok().map(|v| v != 0).unwrap_or(false);
        let display_title = if title.trim().is_empty() { id.clone() } else { title };

        let thumb_url = {
            let dir_path = final_hls_dir(&id);
            dir_path.join("thumb.jpg").exists().then(|| format!("/hls/{id}/thumb.jpg"))
        };

        items.push(VideoItem {
            id,
            title: display_title,
            status,
            progress: Some(100.0),
            error: None,
            thumbnail: thumb_url,
            duration_seconds,
            width,
            height,
            created_at,
            series_id,
            episode_number,
            watch_position,
            watch_completed,
        });
    }

    Ok(items)
}

async fn bulk_delete_videos(pool: &SqlitePool, video_ids: &[String]) -> AppResult<i64> {
    let mut deleted = 0i64;
    for id in video_ids {
        let final_dir = final_hls_dir(id);
        let processing_dir = processing_hls_dir(id);
        let upload_path = upload_dir().join(format!("{id}.mp4"));
        let log_path = processing_log_path(id);
        let lock_path = processing_lock_path(id);

        let _ = fs::remove_dir_all(final_dir);
        let _ = fs::remove_dir_all(processing_dir);
        let _ = fs::remove_file(upload_path);
        let _ = fs::remove_file(log_path);
        let _ = fs::remove_file(lock_path);

        if delete_video_record(pool, id).await.is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

fn processing_log_path(id: &str) -> PathBuf {
    logs_dir().join(format!("{id}.log"))
}

fn processing_lock_path(id: &str) -> PathBuf {
    locks_dir().join(format!("{id}.lock"))
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    age.as_secs() > PROCESSING_LOCK_STALE_SECS
}

fn acquire_processing_lock(id: &str) -> bool {
    let path = processing_lock_path(id);
    if path.exists() && lock_is_stale(&path) {
        let _ = fs::remove_file(&path);
    }
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .is_ok()
}

fn release_processing_lock(id: &str) {
    let path = processing_lock_path(id);
    let _ = fs::remove_file(path);
}

fn sanitize_log_value(value: &str) -> String {
    value.replace('\n', " ").replace('\r', " ")
}

fn write_processing_log(id: &str, level: &str, message: &str, detail: &str) -> std::io::Result<()> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let line = format!(
        "ts={ts} level={level} id={id} user_message={} detail={}\n",
        sanitize_log_value(message),
        sanitize_log_value(detail)
    );
    let path = processing_log_path(id);
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    use std::io::Write;
    file.write_all(line.as_bytes())
}


struct MediaMetadata {
    duration_seconds: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    has_audio: bool,
}

async fn probe_video_metadata(input_mp4: &Path) -> AppResult<MediaMetadata> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        return Ok(MediaMetadata {
            duration_seconds: None,
            width: None,
            height: None,
            has_audio: true,
        });
    }

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("json")
        .arg(input_mp4)
        .output()
        .await
        .map_err(|e| AppError::Ffmpeg {
            message: format!("ffprobe spawn failed: {e}"),
        })?;

    if !output.status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("ffprobe exited with status: {}", output.status),
        });
    }

    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| AppError::Ffmpeg {
        message: format!("ffprobe output parse failed: {e}"),
    })?;

    let duration_seconds = value
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|d| d.round() as i64);

    let (width, height) = value
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.get(0))
        .map(|stream| {
            let width = stream.get("width").and_then(|v| v.as_i64());
            let height = stream.get("height").and_then(|v| v.as_i64());
            (width, height)
        })
        .unwrap_or((None, None));

    let has_audio = value
        .get("streams")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter().any(|stream| {
                stream
                    .get("codec_type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "audio")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    Ok(MediaMetadata {
        duration_seconds,
        width,
        height,
        has_audio,
    })
}

#[get("/static/vendor/hls.min.js")]
async fn hls_js() -> Option<(ContentType, NamedFile)> {
    let path = Path::new("static/vendor/hls.min.js");
    NamedFile::open(path)
        .await
        .ok()
        .map(|file| (ContentType::JavaScript, file))
}

#[get("/static/vendor/hls.min.js.map")]
async fn hls_js_map() -> Option<(ContentType, NamedFile)> {
    let path = Path::new("static/vendor/hls.min.js.map");
    NamedFile::open(path)
        .await
        .ok()
        .map(|file| (ContentType::JSON, file))
}

async fn run_ffmpeg_to_hls(
    input_mp4: &Path,
    out_dir: &Path,
    settings: &HlsSettings,
    metadata: &MediaMetadata,
    db: &SqlitePool,
    id: &str,
) -> AppResult<()> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        let master = out_dir.join("master.m3u8");
        let _ = fs::write(&master, "#EXTM3U\n#EXT-X-VERSION:3\n");
        update_video_progress(db, id, 100.0).await?;
        return Ok(());
    }

    let master = "master.m3u8";
    let segment_pattern = "v%v/seg_%03d.ts";
    let playlist_pattern = "v%v/prog_index.m3u8";

    for (idx, _variant) in settings.variants.iter().enumerate() {
        let variant_dir = out_dir.join(format!("v{idx}"));
        let _ = fs::create_dir_all(&variant_dir);
    }

    let input_abs = fs::canonicalize(input_mp4).map_err(|e| AppError::Storage {
        action: "canonicalizing upload path",
        source: e,
    })?;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostats")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-i")
        .arg(input_abs)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(out_dir);

    let mut filter = String::new();
    if settings.variants.len() > 1 {
        filter.push_str(&format!("[0:v]split={}", settings.variants.len()));
        for idx in 0..settings.variants.len() {
            filter.push_str(&format!("[v{idx}]"));
        }
        filter.push(';');
        for (idx, variant) in settings.variants.iter().enumerate() {
            filter.push_str(&format!(
                "[v{idx}]scale={}:-2[v{idx}out];",
                variant.width
            ));
        }
        filter.pop();
        cmd.arg("-filter_complex").arg(filter);
        for idx in 0..settings.variants.len() {
            cmd.arg("-map").arg(format!("[v{idx}out]"));
            if metadata.has_audio {
                cmd.arg("-map").arg("0:a:0");
            }
        }
    } else if let Some(variant) = settings.variants.first() {
        cmd.arg("-vf").arg(format!("scale={}:-2", variant.width));
        cmd.arg("-map").arg("0:v:0");
        if metadata.has_audio {
            cmd.arg("-map").arg("0:a:0");
        }
    }

    cmd.arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg(&settings.preset)
        .arg("-crf")
        .arg(settings.crf.to_string())
        .arg("-profile:v")
        .arg("main")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", settings.audio_bitrate_k));

    for (idx, variant) in settings.variants.iter().enumerate() {
        cmd.arg(format!("-b:v:{idx}"))
            .arg(format!("{}k", variant.bitrate_k));
    }

    let var_map: String = if metadata.has_audio {
        (0..settings.variants.len())
            .map(|idx| format!("v:{},a:{}", idx * 2, idx * 2 + 1))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        (0..settings.variants.len())
            .map(|idx| format!("v:{idx}"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    cmd.arg("-f")
        .arg("hls")
        .arg("-hls_time")
        .arg(settings.segment_seconds.to_string())
        .arg("-hls_playlist_type")
        .arg("vod")
        .arg("-hls_flags")
        .arg("independent_segments")
        .arg("-hls_segment_filename")
        .arg(segment_pattern)
        .arg("-master_pl_name")
        .arg(master)
        .arg("-var_stream_map")
        .arg(var_map)
        .arg(playlist_pattern);

    let mut child = cmd.spawn().map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg spawn failed: {e}"),
    })?;

    let stdout = child.stdout.take().ok_or_else(|| AppError::Ffmpeg {
        message: "ffmpeg stdout missing".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| AppError::Ffmpeg {
        message: "ffmpeg stderr missing".to_string(),
    })?;
    let stderr_task = task::spawn(async move {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(stderr);
        reader
            .read_to_end(&mut buf)
            .await
            .map(|_| String::from_utf8_lossy(&buf).to_string())
    });
    let mut reader = BufReader::new(stdout).lines();
    let mut last_update = Instant::now();
    let total_ms = metadata.duration_seconds.map(|d| d.saturating_mul(1000) as f64);

    while let Some(line) = reader.next_line().await.map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg progress read failed: {e}"),
    })? {
        if let Some(value) = line.strip_prefix("out_time_ms=") {
            if let Ok(out_ms) = value.trim().parse::<f64>() {
                if let Some(total) = total_ms {
                    let pct = ((out_ms / total) * 100.0).clamp(0.0, 99.9);
                    if last_update.elapsed().as_millis() as u64 >= PROGRESS_UPDATE_INTERVAL_MS {
                        let _ = update_video_progress(db, id, pct).await;
                        last_update = Instant::now();
                    }
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg wait failed: {e}"),
    })?;
    let stderr_output = stderr_task.await.unwrap_or_else(|_| Ok(String::new())).unwrap_or_default();
    if !status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("ffmpeg exited with status: {status}. {stderr_output}"),
        });
    }

    update_video_progress(db, id, 100.0).await?;
    Ok(())
}

async fn generate_thumbnail(input_mp4: &Path, out_dir: &Path) -> AppResult<()> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        let thumb_path = out_dir.join("thumb.jpg");
        let _ = fs::write(thumb_path, []);
        return Ok(());
    }

    let thumb_path = out_dir.join("thumb.jpg");

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg("00:00:01")
        .arg("-i")
        .arg(input_mp4)
        .arg("-vframes")
        .arg("1")
        .arg("-vf")
        .arg("scale=480:-1")
        .arg(&thumb_path)
        .status()
        .await
        .map_err(|e| AppError::Ffmpeg {
            message: format!("ffmpeg spawn failed: {e}"),
        })?;

    if !status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("thumbnail ffmpeg exited with status: {status}"),
        });
    }

    Ok(())
}

async fn process_video(
    id: String,
    upload_path: PathBuf,
    processing_dir: PathBuf,
    final_dir: PathBuf,
    db: SqlitePool,
    settings: HlsSettings,
    retry: RetrySettings,
) {
    if let Err(e) = ensure_dirs() {
        let _ = write_processing_log(&id, "ERROR", &e.user_message(), &e.detail_message());
        let _ = mark_video_error(&db, &id, &e.user_message(), &e.detail_message()).await;
        return;
    }

    if !acquire_processing_lock(&id) {
        return;
    }

    if final_dir.join("master.m3u8").exists() {
        let _ = mark_video_ready(&db, &id).await;
        release_processing_lock(&id);
        return;
    }

    if !upload_path.exists() {
        let message = "Original upload missing; cannot process.";
        let _ = write_processing_log(&id, "ERROR", message, "upload file missing");
        let _ = mark_video_error(&db, &id, message, "upload file missing").await;
        release_processing_lock(&id);
        return;
    }

    let _ = update_video_progress(&db, &id, 0.0).await;

    let metadata = match probe_video_metadata(&upload_path).await {
        Ok(metadata) => Some(metadata),
        Err(err) => {
            let _ = write_processing_log(&id, "WARN", "Metadata probe failed.", &err.detail_message());
            None
        }
    };

    if processing_dir.exists() {
        let _ = fs::remove_dir_all(&processing_dir);
    }
    if let Err(e) = fs::create_dir_all(&processing_dir) {
        let app_error = AppError::Storage {
            action: "creating processing directory",
            source: e,
        };
        let _ = write_processing_log(&id, "ERROR", &app_error.user_message(), &app_error.detail_message());
        let _ = mark_video_error(&db, &id, &app_error.user_message(), &app_error.detail_message()).await;
        release_processing_lock(&id);
        return;
    }
    if let Some(ref metadata) = metadata {
        let _ = update_video_metadata(
            &db,
            &id,
            metadata.duration_seconds,
            metadata.width,
            metadata.height,
        )
        .await;
    }

    let thumb_result = generate_thumbnail(&upload_path, &processing_dir).await;
    let default_meta = MediaMetadata {
        duration_seconds: None,
        width: None,
        height: None,
        has_audio: true,
    };
    let run_meta = metadata.as_ref().unwrap_or(&default_meta);
    if let Err(e) = run_ffmpeg_to_hls(
        &upload_path,
        &processing_dir,
        &settings,
        run_meta,
        &db,
        &id,
    )
    .await
    {
        let _ = write_processing_log(&id, "ERROR", &e.user_message(), &e.detail_message());
        let _ = mark_video_error(&db, &id, &e.user_message(), &e.detail_message()).await;
        let _ = fs::remove_dir_all(&processing_dir);
        release_processing_lock(&id);
        let _ = schedule_retry(&id, &db, settings.clone(), retry).await;
        return;
    }

    if final_dir.exists() {
        let _ = fs::remove_dir_all(&final_dir);
    }
    if let Err(e) = fs::rename(&processing_dir, &final_dir) {
        let app_error = AppError::Storage {
            action: "finalizing HLS output directory",
            source: e,
        };
        let _ = write_processing_log(&id, "ERROR", &app_error.user_message(), &app_error.detail_message());
        let _ = mark_video_error(&db, &id, &app_error.user_message(), &app_error.detail_message()).await;
        let _ = fs::remove_dir_all(&processing_dir);
        release_processing_lock(&id);
        return;
    }
    let _ = mark_video_ready(&db, &id).await;
    release_processing_lock(&id);

    if let Err(e) = thumb_result {
        let _ = write_processing_log(
            &id,
            "WARN",
            "Thumbnail generation failed. The video might be unsupported.",
            &e.detail_message(),
        );
    }
}

async fn schedule_retry(
    id: &str,
    db: &SqlitePool,
    settings: HlsSettings,
    retry: RetrySettings,
) -> AppResult<()> {
    let retries = bump_retry_count(db, id).await?;
    if retries > retry.max_retries {
        return Ok(());
    }
    reset_for_reprocess(db, id).await?;
    let id = id.to_string();
    let db = db.clone();
    task::spawn(async move {
        sleep(Duration::from_secs(retry.delay_secs)).await;
        spawn_processing_task(id, db, settings, retry);
    });
    Ok(())
}

fn spawn_processing_task(id: String, db: SqlitePool, settings: HlsSettings, retry: RetrySettings) {
    let upload_path = upload_dir().join(format!("{id}.mp4"));
    let processing_dir = processing_hls_dir(&id);
    let final_dir = final_hls_dir(&id);
    task::spawn(process_video(
        id,
        upload_path,
        processing_dir,
        final_dir,
        db,
        settings,
        retry,
    ));
}

async fn ensure_video_record(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let title = id.to_string();
    insert_video(pool, id, &title).await
}

async fn scan_uploads_for_jobs(pool: &SqlitePool) -> AppResult<Vec<String>> {
    ensure_dirs()?;
    let mut ids = Vec::new();
    let entries = fs::read_dir(upload_dir()).map_err(|e| AppError::Storage {
        action: "reading upload directory",
        source: e,
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("mp4") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = stem.to_string();
        let lock_path = processing_lock_path(&id);
        if lock_path.exists() {
            if lock_is_stale(&lock_path) {
                let _ = fs::remove_file(&lock_path);
            } else {
                continue;
            }
        }
        let final_dir = final_hls_dir(&id);
        let processing_dir = processing_hls_dir(&id);
        if final_dir.join("master.m3u8").exists() {
            let _ = mark_video_ready(pool, &id).await;
            continue;
        }
        if processing_dir.exists() {
            let _ = fs::remove_dir_all(&processing_dir);
        }
        let _ = ensure_video_record(pool, &id).await;
        ids.push(id);
    }

    Ok(ids)
}

async fn reconcile_processing_jobs(pool: &SqlitePool) -> AppResult<Vec<String>> {
    let ids = list_processing_ids(pool).await?;
    let mut resume_ids = Vec::new();
    for id in ids {
        let upload_path = upload_dir().join(format!("{id}.mp4"));
        let final_dir = final_hls_dir(&id);
        let processing_dir = processing_hls_dir(&id);
        let master = final_dir.join("master.m3u8");
        let lock_path = processing_lock_path(&id);
        if lock_path.exists() {
            let _ = fs::remove_file(&lock_path);
        }

        if master.exists() {
            let _ = fs::remove_dir_all(&processing_dir);
            let _ = mark_video_ready(pool, &id).await;
            continue;
        }

        if upload_path.exists() {
            resume_ids.push(id);
            continue;
        }

        if processing_dir.exists() {
            let _ = fs::remove_dir_all(&processing_dir);
        }
        let message = "Original upload missing; cannot resume processing.";
        let _ = write_processing_log(&id, "ERROR", message, "upload file missing");
        let _ = mark_video_error(pool, &id, message, "upload file missing").await;
    }

    Ok(resume_ids)
}

#[derive(FromForm)]
struct VideosQuery<'r> {
    search: Option<&'r str>,
}

#[get("/api/videos?<query..>")]
async fn list_videos_api(state: &State<AppState>, query: VideosQuery<'_>) -> Json<Vec<VideoItem>> {
    let videos = match list_videos(&state.db, query.search).await {
        Ok(videos) if !videos.is_empty() => videos,
        Ok(_) | Err(_) => list_videos_from_fs(),
    };
    Json(videos)
}

#[get("/api/videos/continue")]
async fn continue_watching_api(state: &State<AppState>) -> Result<Json<Vec<VideoItem>>, (Status, Json<ApiError>)> {
    let videos = get_continue_watching(&state.db).await.map_err(to_api_error)?;
    Ok(Json(videos))
}

#[get("/api/status/<id>")]
async fn status_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<StatusResponse>, (Status, Json<ApiError>)> {
    match read_video_status_full(&state.db, id).await.map_err(to_api_error)? {
        Some(status) => Ok(Json(status)),
        None => Err((Status::NotFound, Json(ApiError { message: "Video not found.".to_string() }))),
    }
}

#[post("/api/video/<id>/rename", data = "<req>")]
async fn rename_video(
    state: &State<AppState>,
    id: &str,
    req: Json<RenameRequest>,
) -> Result<Json<StatusResponse>, (Status, Json<ApiError>)> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err((Status::BadRequest, Json(ApiError { message: "Title cannot be empty.".to_string() })));
    }

    update_video_title(&state.db, id, title).await.map_err(to_api_error)?;
    let status = read_video_status_full(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Video not found.".to_string() })))?;
    Ok(Json(status))
}

#[post("/api/video/<id>/reprocess")]
async fn reprocess_video(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<StatusResponse>, (Status, Json<ApiError>)> {
    let upload_path = upload_dir().join(format!("{id}.mp4"));
    if !upload_path.exists() {
        return Err((Status::BadRequest, Json(ApiError { message: "Original upload missing; cannot reprocess.".to_string() })));
    }

    reset_for_reprocess(&state.db, id).await.map_err(to_api_error)?;
    spawn_processing_task(id.to_string(), state.db.clone(), state.hls.clone(), state.retry);

    let status = read_video_status_full(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Video not found.".to_string() })))?;
    Ok(Json(status))
}

#[delete("/api/video/<id>")]
async fn delete_video(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<serde_json::Value>, (Status, Json<ApiError>)> {
    let final_dir = final_hls_dir(id);
    let processing_dir = processing_hls_dir(id);
    let upload_path = upload_dir().join(format!("{id}.mp4"));
    let log_path = processing_log_path(id);
    let lock_path = processing_lock_path(id);

    let _ = fs::remove_dir_all(final_dir);
    let _ = fs::remove_dir_all(processing_dir);
    let _ = fs::remove_file(upload_path);
    let _ = fs::remove_file(log_path);
    let _ = fs::remove_file(lock_path);

    delete_video_record(&state.db, id).await.map_err(to_api_error)?;
    Ok(Json(json!({ "ok": true })))
}

#[get("/api/series")]
async fn list_series_api(state: &State<AppState>) -> Result<Json<Vec<SeriesItem>>, (Status, Json<ApiError>)> {
    let series = list_series(&state.db).await.map_err(to_api_error)?;
    Ok(Json(series))
}

#[post("/api/series", data = "<req>")]
async fn create_series_api(
    state: &State<AppState>,
    req: Json<CreateSeriesRequest>,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err((Status::BadRequest, Json(ApiError { message: "Title cannot be empty.".to_string() })));
    }

    let series_id = create_series(&state.db, title, req.description.as_deref()).await.map_err(to_api_error)?;

    if let Some(ref video_ids) = req.video_ids {
        if !video_ids.is_empty() {
            add_videos_to_series(&state.db, &series_id, video_ids).await.map_err(to_api_error)?;
        }
    }

    let detail = get_series_detail(&state.db, &series_id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::InternalServerError, Json(ApiError { message: "Series creation failed.".to_string() })))?;

    Ok(Json(detail))
}

#[get("/api/series/<id>")]
async fn get_series_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    let detail = get_series_detail(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[put("/api/series/<id>", data = "<req>")]
async fn update_series_api(
    state: &State<AppState>,
    id: &str,
    req: Json<UpdateSeriesRequest>,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    update_series_info(
        &state.db,
        id,
        req.title.as_deref(),
        req.description.as_deref(),
        req.cover_video_id.as_deref(),
    )
    .await
    .map_err(to_api_error)?;

    let detail = get_series_detail(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[delete("/api/series/<id>")]
async fn delete_series_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<serde_json::Value>, (Status, Json<ApiError>)> {
    delete_series(&state.db, id).await.map_err(to_api_error)?;
    Ok(Json(json!({ "ok": true })))
}

#[post("/api/series/<id>/videos", data = "<req>")]
async fn add_videos_to_series_api(
    state: &State<AppState>,
    id: &str,
    req: Json<AddVideosToSeriesRequest>,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    add_videos_to_series(&state.db, id, &req.video_ids).await.map_err(to_api_error)?;

    let detail = get_series_detail(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[delete("/api/series/<series_id>/videos/<video_id>")]
async fn remove_video_from_series_api(
    state: &State<AppState>,
    series_id: &str,
    video_id: &str,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    remove_video_from_series(&state.db, video_id).await.map_err(to_api_error)?;

    let detail = get_series_detail(&state.db, series_id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[put("/api/series/<id>/reorder", data = "<req>")]
async fn reorder_series_api(
    state: &State<AppState>,
    id: &str,
    req: Json<ReorderEpisodesRequest>,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    reorder_series_episodes(&state.db, id, &req.video_ids).await.map_err(to_api_error)?;

    let detail = get_series_detail(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[put("/api/video/<id>/progress", data = "<req>")]
async fn update_progress_api(
    state: &State<AppState>,
    id: &str,
    req: Json<UpdateProgressRequest>,
) -> Result<Json<WatchProgressResponse>, (Status, Json<ApiError>)> {
    let completed = req.completed.unwrap_or(false);
    update_watch_progress(&state.db, id, req.position_seconds, completed).await.map_err(to_api_error)?;

    Ok(Json(WatchProgressResponse {
        video_id: id.to_string(),
        position_seconds: req.position_seconds,
        completed,
    }))
}

#[get("/api/video/<id>/progress")]
async fn get_progress_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<WatchProgressResponse>, (Status, Json<ApiError>)> {
    let progress = get_watch_progress(&state.db, id)
        .await
        .map_err(to_api_error)?
        .unwrap_or(WatchProgressResponse {
            video_id: id.to_string(),
            position_seconds: 0,
            completed: false,
        });

    Ok(Json(progress))
}

#[post("/api/videos/bulk/delete", data = "<req>")]
async fn bulk_delete_api(
    state: &State<AppState>,
    req: Json<BulkDeleteRequest>,
) -> Result<Json<serde_json::Value>, (Status, Json<ApiError>)> {
    let deleted = bulk_delete_videos(&state.db, &req.video_ids).await.map_err(to_api_error)?;
    Ok(Json(json!({ "ok": true, "deleted": deleted })))
}

#[post("/api/videos/bulk/add-to-series", data = "<req>")]
async fn bulk_add_to_series_api(
    state: &State<AppState>,
    req: Json<BulkAddToSeriesRequest>,
) -> Result<Json<SeriesDetail>, (Status, Json<ApiError>)> {
    add_videos_to_series(&state.db, &req.series_id, &req.video_ids).await.map_err(to_api_error)?;

    let detail = get_series_detail(&state.db, &req.series_id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Series not found.".to_string() })))?;

    Ok(Json(detail))
}

#[post("/upload", data = "<form>")]
async fn upload_post(
    state: &State<AppState>,
    mut form: Form<UploadForm<'_>>,
) -> Result<Json<UploadResult>, (Status, Json<ApiError>)> {
    ensure_dirs().map_err(to_api_error)?;

    let id = Uuid::new_v4().to_string();
    let title = form.title.unwrap_or("").trim().to_string();

    let upload_path = upload_dir().join(format!("{id}.mp4"));
    let out_dir = processing_hls_dir(&id);

    fs::create_dir_all(&out_dir).map_err(|e| {
        to_api_error(AppError::Storage {
            action: "creating output directory",
            source: e,
        })
    })?;

    if let Err(e) = form.video.persist_to(&upload_path).await {
        if e.raw_os_error() == Some(18) {
            // Fallback to copy when temp storage and data dir are on different volumes.
            form.video
                .copy_to(&upload_path)
                .await
                .map_err(|e| {
                    to_api_error(AppError::Upload {
                        message: format!("Saving the upload failed: {e}"),
                    })
                })?;
        } else {
            return Err(to_api_error(AppError::Upload {
                message: format!("Saving the upload failed: {e}"),
            }));
        }
    }

    let title_for_db = if title.trim().is_empty() { id.clone() } else { title.clone() };
    insert_video(&state.db, &id, &title_for_db).await.map_err(to_api_error)?;

    // Spawn background processing so the request returns immediately.
    spawn_processing_task(id.clone(), state.db.clone(), state.hls.clone(), state.retry);

    Ok(Json(UploadResult {
        id: id.clone(),
        title,
        watch_url: format!("/watch/{id}"),
    }))
}


fn frontend_dist_dir() -> PathBuf {
    env::var("FRONTEND_DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"))
}

#[get("/")]
async fn index_page() -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/upload")]
async fn upload_page() -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/watch/<_id>")]
async fn watch_page(_id: &str) -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/series/<_id>")]
async fn series_page(_id: &str) -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/series/<_id>/manage")]
async fn series_manage_page(_id: &str) -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[catch(404)]
async fn not_found(_req: &Request<'_>) -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[catch(413)]
fn payload_too_large() -> Json<ApiError> {
    Json(ApiError {
        message: "Upload too large. Try a smaller file or adjust server limits.".to_string(),
    })
}

#[catch(500)]
fn server_error() -> Json<ApiError> {
    Json(ApiError {
        message: "Server error. Please try again later.".to_string(),
    })
}

#[launch]
fn rocket() -> _ {
    ensure_dirs().ok();

    let hls_path = hls_dir();
    let frontend_assets = frontend_dist_dir().join("assets");

    rocket::build()
        .mount(
            "/",
            routes![
                index_page,
                upload_page,
                upload_post,
                watch_page,
                series_page,
                series_manage_page,
                list_videos_api,
                continue_watching_api,
                status_api,
                rename_video,
                reprocess_video,
                delete_video,
                list_series_api,
                create_series_api,
                get_series_api,
                update_series_api,
                delete_series_api,
                add_videos_to_series_api,
                remove_video_from_series_api,
                reorder_series_api,
                update_progress_api,
                get_progress_api,
                bulk_delete_api,
                bulk_add_to_series_api,
                hls_js,
                hls_js_map
            ],
        )
        .mount("/hls", FileServer::from(hls_path))
        .mount("/assets", FileServer::from(frontend_assets))
        .attach(AdHoc::on_ignite("SQLite", |rocket| async {
            let db = init_db().await.expect("failed to initialize sqlite");
            rocket.manage(AppState {
                db,
                hls: hls_settings(),
                retry: retry_settings(),
            })
        }))
        .attach(AdHoc::on_liftoff("ResumeProcessing", |rocket| {
            Box::pin(async move {
                let Some(state) = rocket.state::<AppState>() else {
                    return;
                };

                if let Ok(ids) = reconcile_processing_jobs(&state.db).await {
                    for id in ids {
                        spawn_processing_task(id, state.db.clone(), state.hls.clone(), state.retry);
                    }
                }
            })
        }))
        .attach(AdHoc::on_liftoff("WatchUploads", |rocket| {
            Box::pin(async move {
                let Some(state) = rocket.state::<AppState>() else {
                    return;
                };

                let pool = state.db.clone();
                let hls = state.hls.clone();
                let retry = state.retry;
                task::spawn(async move {
                    let mut ticker = rocket::tokio::time::interval(Duration::from_secs(UPLOAD_SCAN_INTERVAL_SECS));
                    loop {
                        ticker.tick().await;
                        if let Ok(ids) = scan_uploads_for_jobs(&pool).await {
                            for id in ids {
                                spawn_processing_task(id, pool.clone(), hls.clone(), retry);
                            }
                        }
                    }
                });
            })
        }))
        .attach(AdHoc::on_liftoff("DevFrontend", |_rocket| {
            Box::pin(async move {
                let frontend_dir = PathBuf::from("frontend");

                task::spawn(async move {
                    let result = Command::new("npm")
                        .arg("run")
                        .arg("dev")
                        .current_dir(&frontend_dir)
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .spawn();

                    match result {
                        Ok(mut child) => {
                            let _ = child.wait().await;
                        }
                        Err(e) => {
                            eprintln!("Failed to start frontend dev server: {e}");
                        }
                    }
                });
            })
        }))
        .register("/", catchers![not_found, payload_too_large, server_error])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Status};
    use rocket::local::asynchronous::Client;
    use serde_json::Value;
    use std::time::Duration;
    use tempfile::TempDir;

    fn build_multipart(title: &str, filename: &str, content: &str, boundary: &str) -> Vec<u8> {
        let mut body = String::new();
        body.push_str(&format!("--{boundary}\r\n"));
        body.push_str("Content-Disposition: form-data; name=\"title\"\r\n\r\n");
        body.push_str(title);
        body.push_str("\r\n");
        body.push_str(&format!("--{boundary}\r\n"));
        body.push_str(&format!(
            "Content-Disposition: form-data; name=\"video\"; filename=\"{filename}\"\r\n"
        ));
        body.push_str("Content-Type: video/mp4\r\n\r\n");
        body.push_str(content);
        body.push_str("\r\n");
        body.push_str(&format!("--{boundary}--\r\n"));
        body.into_bytes()
    }

    #[rocket::async_test]
    async fn test_upload_watch_flow() {
        let temp = TempDir::new().expect("tempdir");
        env::set_var("DATA_DIR", temp.path());
        env::set_var("SQLITE_PATH", temp.path().join("app.db"));
        env::set_var("FFMPEG_DRY_RUN", "1");

        let client = Client::tracked(rocket()).await.expect("client");

        let boundary = "BOUNDARY123";
        let body = build_multipart("Test Video", "video.mp4", "dummy-video", boundary);
        let resp = client
            .post("/upload")
            .header(ContentType::with_params("multipart", "form-data", ("boundary", boundary)))
            .body(body)
            .dispatch()
            .await;

        assert_eq!(resp.status(), Status::Ok);
        let data: Value = resp.into_json().await.expect("json");
        let id = data["id"].as_str().expect("id");

        let mut ready = false;
        for _ in 0..20 {
            let status_resp = client.get(format!("/api/status/{id}")).dispatch().await;
            assert!(status_resp.status().class().is_success());
            let status: Value = status_resp.into_json().await.expect("status json");
            if status["status"] == "ready" {
                ready = true;
                break;
            }
            rocket::tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(ready, "video should become ready");

        let watch = client.get(format!("/watch/{id}")).dispatch().await;
        assert_eq!(watch.status(), Status::Ok);

        let rename = client
            .post(format!("/api/video/{id}/rename"))
            .header(ContentType::JSON)
            .body(r#"{"title":"Renamed"}"#)
            .dispatch()
            .await;
        assert_eq!(rename.status(), Status::Ok);

        let delete = client.delete(format!("/api/video/{id}")).dispatch().await;
        assert_eq!(delete.status(), Status::Ok);

        let missing = client.get(format!("/api/status/{id}")).dispatch().await;
        assert_eq!(missing.status(), Status::NotFound);
    }
}
