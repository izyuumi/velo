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
use rocket_dyn_templates::{context, Template};
use serde::Serialize;
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::{env, fs};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::error::{AppError, AppResult, ApiError, to_api_error};

const STATUS_PROCESSING: &str = "processing";
const STATUS_READY: &str = "ready";
const STATUS_ERROR: &str = "error";
const MAX_LIST_VIDEOS: i64 = 200;

struct AppState {
    db: SqlitePool,
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

fn sqlite_path() -> PathBuf {
    env::var("SQLITE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_root().join("app.db"))
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

#[derive(Serialize)]
struct VideoItem {
    id: String,
    title: String,
    thumbnail: Option<String>,
}

#[derive(Serialize)]
struct WatchContext {
    id: String,
    ready: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct UploadResult {
    id: String,
    title: String,
    watch_url: String,
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

        let title_path = dir_path.join("title.txt");
        let title = fs::read_to_string(&title_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.clone());

        let master_exists = dir_path.join("master.m3u8").exists();
        if master_exists {
            let thumb_url = dir_path
                .join("thumb.jpg")
                .exists()
                .then(|| format!("/hls/{id}/thumb.jpg"));

            items.push(VideoItem { id, title, thumbnail: thumb_url });
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

    Ok(())
}

async fn upsert_ready_video(pool: &SqlitePool, id: &str, title: &str, created_at: i64) -> AppResult<()> {
    let now = unix_ts();
    sqlx::query(
        "INSERT INTO videos (id, title, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE
         SET title=excluded.title,
             status=excluded.status,
             updated_at=excluded.updated_at,
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

        let master_exists = dir_path.join("master.m3u8").exists();
        if !master_exists {
            continue;
        }

        let title_path = dir_path.join("title.txt");
        let title = fs::read_to_string(&title_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string());

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
        "INSERT INTO videos (id, title, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE
         SET title=excluded.title,
             status=excluded.status,
             updated_at=excluded.updated_at,
             error_message=NULL,
             error_detail=NULL",
    )
    .bind(id)
    .bind(title)
    .bind(STATUS_PROCESSING)
    .bind(now)
    .bind(now)
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

async fn list_videos(pool: &SqlitePool) -> AppResult<Vec<VideoItem>> {
    let rows = sqlx::query(
        "SELECT id, title
         FROM videos
         WHERE status = ?
         ORDER BY created_at DESC
         LIMIT ?",
    )
    .bind(STATUS_READY)
    .bind(MAX_LIST_VIDEOS)
    .fetch_all(pool)
    .await
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
        let display_title = if title.trim().is_empty() { id.clone() } else { title };

        let dir_path = hls_dir().join(&id);
        let master_exists = dir_path.join("master.m3u8").exists();
        if !master_exists {
            let _ = mark_video_error(
                pool,
                &id,
                "Video package missing on disk.",
                "HLS output is missing master.m3u8.",
            )
            .await;
            continue;
        }

        let thumb_url = dir_path
            .join("thumb.jpg")
            .exists()
            .then(|| format!("/hls/{id}/thumb.jpg"));

        items.push(VideoItem {
            id,
            title: display_title,
            thumbnail: thumb_url,
        });
    }

    Ok(items)
}

async fn read_video_status(pool: &SqlitePool, id: &str) -> AppResult<Option<(String, Option<String>)>> {
    let row = sqlx::query("SELECT status, error_message FROM videos WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database {
            action: "reading video status",
            source: e,
        })?;

    if let Some(row) = row {
        let status: String = row.try_get("status").unwrap_or_default();
        let error_message: Option<String> = row.try_get("error_message").ok();
        return Ok(Some((status, error_message)));
    }

    Ok(None)
}

fn processing_log_path(id: &str) -> PathBuf {
    logs_dir().join(format!("{id}.log"))
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

fn read_processing_error(id: &str) -> Option<String> {
    let contents = fs::read_to_string(processing_log_path(id)).ok()?;
    for line in contents.lines().rev() {
        if !line.contains("level=ERROR") {
            continue;
        }
        if let Some(start) = line.find("user_message=") {
            let msg_start = start + "user_message=".len();
            let remainder = &line[msg_start..];
            if let Some(end) = remainder.find(" detail=") {
                return Some(remainder[..end].trim().to_string());
            }
            return Some(remainder.trim().to_string());
        }
    }
    None
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

async fn run_ffmpeg_to_hls(input_mp4: &Path, out_dir: &Path) -> AppResult<()> {
    let master = out_dir.join("master.m3u8");
    let segment_pattern = out_dir.join("seg_%03d.ts");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input_mp4)
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("23")
        .arg("-profile:v")
        .arg("main")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("128k")
        .arg("-f")
        .arg("hls")
        .arg("-hls_time")
        .arg("4")
        .arg("-hls_playlist_type")
        .arg("vod")
        .arg("-hls_flags")
        .arg("independent_segments")
        .arg("-hls_segment_filename")
        .arg(segment_pattern)
        .arg(master);

    let status = cmd
        .status()
        .await
        .map_err(|e| AppError::Ffmpeg {
            message: format!("ffmpeg spawn failed: {e}"),
        })?;

    if !status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("ffmpeg exited with status: {status}"),
        });
    }

    Ok(())
}

async fn generate_thumbnail(input_mp4: &Path, out_dir: &Path) -> AppResult<()> {
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

#[get("/")]
async fn index(state: &State<AppState>) -> Template {
    let videos = match list_videos(&state.db).await {
        Ok(videos) if !videos.is_empty() => videos,
        Ok(_) | Err(_) => list_videos_from_fs(),
    };
    Template::render("index", context! { videos })
}

#[get("/upload")]
fn upload_page() -> Template {
    Template::render("upload", context! {})
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
    let out_dir = hls_dir().join(&id);

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

    if !title.is_empty() {
        let _ = fs::write(out_dir.join("title.txt"), &title);
    }

    let title_for_db = if title.trim().is_empty() { id.clone() } else { title.clone() };
    insert_video(&state.db, &id, &title_for_db).await.map_err(to_api_error)?;

    // Spawn background processing so the request returns immediately.
    let upload_path_bg = upload_path.clone();
    let out_dir_bg = out_dir.clone();
    let id_bg = id.clone();
    let db_bg = state.db.clone();
    task::spawn(async move {
        let thumb_result = generate_thumbnail(&upload_path_bg, &out_dir_bg).await;
        if let Err(e) = run_ffmpeg_to_hls(&upload_path_bg, &out_dir_bg).await {
            let _ = write_processing_log(&id_bg, "ERROR", &e.user_message(), &e.detail_message());
            let _ = mark_video_error(&db_bg, &id_bg, &e.user_message(), &e.detail_message()).await;
        } else {
            // Clean up original upload after successful packaging to save space.
            let _ = fs::remove_file(&upload_path_bg);
            let _ = mark_video_ready(&db_bg, &id_bg).await;
        }

        if let Err(e) = thumb_result {
            let _ = write_processing_log(
                &id_bg,
                "WARN",
                "Thumbnail generation failed. The video might be unsupported.",
                &e.detail_message(),
            );
        }
    });

    Ok(Json(UploadResult {
        id: id.clone(),
        title,
        watch_url: format!("/watch/{id}"),
    }))
}

#[get("/watch/<id>")]
async fn watch(state: &State<AppState>, id: &str) -> Template {
    let id_owned = id.to_string();
    let out_dir = hls_dir().join(id);
    let master = out_dir.join("master.m3u8");

    let ready = master.exists();
    let mut error = read_processing_error(id);
    if let Ok(Some((status, db_error))) = read_video_status(&state.db, id).await {
        if status == STATUS_ERROR {
            error = db_error.or(error);
        }
    }

    let ctx = WatchContext { id: id_owned, ready, error };
    Template::render("watch", context! { ctx })
}

#[catch(404)]
fn not_found(req: &Request<'_>) -> Template {
    Template::render(
        "error",
        context! {
            code: 404,
            title: "Not Found",
            message: format!("We couldn't find {}", req.uri()),
        },
    )
}

#[catch(413)]
fn payload_too_large() -> Template {
    Template::render(
        "error",
        context! {
            code: 413,
            title: "Upload Too Large",
            message: "That file is larger than the server allows. Try a smaller upload or adjust Rocket.toml limits.",
        },
    )
}

#[catch(500)]
fn server_error() -> Template {
    Template::render(
        "error",
        context! {
            code: 500,
            title: "Server Error",
            message: "Something went wrong on the server. Please try again in a moment.",
        },
    )
}

#[launch]
fn rocket() -> _ {
    ensure_dirs().ok();

    let hls_path = hls_dir();
    let static_path = PathBuf::from("static");

    rocket::build()
        .mount("/", routes![index, upload_page, upload_post, watch, hls_js, hls_js_map])
        .mount("/hls", FileServer::from(hls_path))
        .mount("/static", FileServer::from(static_path))
        .attach(Template::fairing())
        .attach(AdHoc::on_ignite("SQLite", |rocket| async {
            let db = init_db().await.expect("failed to initialize sqlite");
            rocket.manage(AppState { db })
        }))
        .register("/", catchers![not_found, payload_too_large, server_error])
}
