use crate::config::{
    final_hls_dir, hls_dir, upload_dir, MAX_LIST_VIDEOS, PROCESSING_DIR_SUFFIX,
    STATUS_ERROR, STATUS_PROCESSING, STATUS_READY,
};
use crate::error::{AppError, AppResult};
use crate::types::{StatusResponse, VideoItem};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn system_time_to_unix(ts: SystemTime) -> i64 {
    ts.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn upsert_ready_video(pool: &SqlitePool, id: &str, title: &str, created_at: i64) -> AppResult<()> {
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

pub async fn sync_db_from_fs(pool: &SqlitePool) -> AppResult<()> {
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

pub async fn insert_video(pool: &SqlitePool, id: &str, title: &str) -> AppResult<()> {
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

pub async fn mark_video_ready(pool: &SqlitePool, id: &str) -> AppResult<()> {
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

pub async fn mark_video_error(pool: &SqlitePool, id: &str, message: &str, detail: &str) -> AppResult<()> {
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

pub async fn update_video_progress(pool: &SqlitePool, id: &str, progress: f64) -> AppResult<()> {
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

pub async fn update_video_metadata(
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

pub async fn update_video_title(pool: &SqlitePool, id: &str, title: &str) -> AppResult<()> {
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

pub async fn bump_retry_count(pool: &SqlitePool, id: &str) -> AppResult<i64> {
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

pub async fn reset_for_reprocess(pool: &SqlitePool, id: &str) -> AppResult<()> {
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

pub async fn list_videos(pool: &SqlitePool, search: Option<&str>) -> AppResult<Vec<VideoItem>> {
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

pub async fn read_video_status_full(pool: &SqlitePool, id: &str) -> AppResult<Option<StatusResponse>> {
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

pub async fn list_processing_ids(pool: &SqlitePool) -> AppResult<Vec<String>> {
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

pub async fn delete_video_record(pool: &SqlitePool, id: &str) -> AppResult<()> {
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

pub async fn bulk_delete_videos(pool: &SqlitePool, video_ids: &[String]) -> AppResult<i64> {
    use crate::config::{locks_dir, logs_dir, processing_hls_dir};

    let mut deleted = 0i64;
    for id in video_ids {
        let final_dir = final_hls_dir(id);
        let processing_dir = processing_hls_dir(id);
        let upload_path = upload_dir().join(format!("{id}.mp4"));
        let log_path = logs_dir().join(format!("{id}.log"));
        let lock_path = locks_dir().join(format!("{id}.lock"));

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
