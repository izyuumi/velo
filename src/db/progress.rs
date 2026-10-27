use crate::config::{final_hls_dir, STATUS_READY};
use crate::error::{AppError, AppResult};
use crate::types::{VideoItem, WatchProgressResponse};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn update_watch_progress(pool: &SqlitePool, video_id: &str, position: i64, completed: bool) -> AppResult<()> {
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

pub async fn get_watch_progress(pool: &SqlitePool, video_id: &str) -> AppResult<Option<WatchProgressResponse>> {
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

pub async fn get_continue_watching(pool: &SqlitePool) -> AppResult<Vec<VideoItem>> {
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
