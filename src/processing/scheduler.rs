use crate::config::{
    final_hls_dir, locks_dir, logs_dir, max_workers, processing_hls_dir, upload_dir, HlsSettings,
    RetrySettings, PROCESSING_LOCK_STALE_SECS,
};
use crate::db::{
    bump_retry_count, ensure_dirs, insert_video, list_processing_ids, mark_video_error,
    mark_video_ready, reset_for_reprocess, update_video_metadata, update_video_progress,
};
use crate::error::AppError;
use crate::processing::{generate_thumbnail, probe_video_metadata, run_ffmpeg_to_hls, MediaMetadata};
use sqlx::SqlitePool;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use tokio::task;
use tokio::time::sleep;

static PROCESSING_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> Arc<Semaphore> {
    PROCESSING_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(max_workers())))
        .clone()
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
    file.write_all(line.as_bytes())
}

async fn process_video(
    id: String,
    upload_path: PathBuf,
    processing_dir: PathBuf,
    final_dir: PathBuf,
    db: SqlitePool,
    settings: HlsSettings,
    retry: RetrySettings,
    force: bool,
) {
    if let Err(e) = ensure_dirs() {
        let _ = write_processing_log(&id, "ERROR", &e.user_message(), &e.detail_message());
        let _ = mark_video_error(&db, &id, &e.user_message(), &e.detail_message()).await;
        return;
    }

    if !acquire_processing_lock(&id) {
        return;
    }

    if !force && final_dir.join("master.m3u8").exists() {
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
        let _ = schedule_retry(&id, &db, settings.clone(), retry, force).await;
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
    force: bool,
) -> crate::error::AppResult<()> {
    let retries = bump_retry_count(db, id).await?;
    if retries > retry.max_retries {
        return Ok(());
    }
    reset_for_reprocess(db, id).await?;
    let id = id.to_string();
    let db = db.clone();
    task::spawn(async move {
        sleep(Duration::from_secs(retry.delay_secs)).await;
        spawn_task(id, db, settings, retry, force);
    });
    Ok(())
}

pub fn spawn_processing_task(id: String, db: SqlitePool, settings: HlsSettings, retry: RetrySettings) {
    spawn_task(id, db, settings, retry, false);
}

pub fn spawn_reprocessing_task(id: String, db: SqlitePool, settings: HlsSettings, retry: RetrySettings) {
    spawn_task(id, db, settings, retry, true);
}

fn spawn_task(id: String, db: SqlitePool, settings: HlsSettings, retry: RetrySettings, force: bool) {
    let upload_path = upload_dir().join(format!("{id}.mp4"));
    let processing_dir = processing_hls_dir(&id);
    let final_dir = final_hls_dir(&id);
    let semaphore = get_semaphore();

    task::spawn(async move {
        let _permit = semaphore.acquire().await;
        process_video(
            id,
            upload_path,
            processing_dir,
            final_dir,
            db,
            settings,
            retry,
            force,
        )
        .await;
    });
}

pub async fn scan_uploads_for_jobs(pool: &SqlitePool) -> crate::error::AppResult<Vec<String>> {
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
        let _ = insert_video(pool, &id, &id).await;
        ids.push(id);
    }

    Ok(ids)
}

pub async fn reconcile_processing_jobs(pool: &SqlitePool) -> crate::error::AppResult<Vec<String>> {
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
