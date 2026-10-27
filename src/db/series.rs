use crate::config::{final_hls_dir, STATUS_PROCESSING, STATUS_READY};
use crate::error::{AppError, AppResult};
use crate::types::{SeriesDetail, SeriesItem, VideoItem};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn list_series(pool: &SqlitePool) -> AppResult<Vec<SeriesItem>> {
    let rows = sqlx::query(
        "SELECT s.id, s.title, s.description,
                COALESCE(s.cover_video_id, (
                    SELECT id FROM videos
                    WHERE series_id = s.id AND status = 'ready'
                    ORDER BY episode_number ASC LIMIT 1
                )) AS cover_video_id,
                s.created_at, s.updated_at,
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

pub async fn get_series_detail(pool: &SqlitePool, id: &str) -> AppResult<Option<SeriesDetail>> {
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

pub async fn get_series_episodes(pool: &SqlitePool, series_id: &str) -> AppResult<Vec<VideoItem>> {
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

pub async fn create_series(pool: &SqlitePool, title: &str, description: Option<&str>) -> AppResult<String> {
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

pub async fn update_series_info(
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

pub async fn delete_series(pool: &SqlitePool, id: &str) -> AppResult<()> {
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

pub async fn add_videos_to_series(pool: &SqlitePool, series_id: &str, video_ids: &[String]) -> AppResult<()> {
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

pub async fn remove_video_from_series(pool: &SqlitePool, video_id: &str) -> AppResult<()> {
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

pub async fn reorder_series_episodes(pool: &SqlitePool, series_id: &str, video_ids: &[String]) -> AppResult<()> {
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
