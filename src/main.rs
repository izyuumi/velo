#[macro_use]
extern crate rocket;

mod error;

use rocket::form::Form;
use rocket::fs::{FileServer, NamedFile, TempFile};
use rocket::http::{ContentType, Status};
use rocket::Request;
use rocket::serde::json::Json;
use rocket::tokio::process::Command;
use rocket::tokio::task;
use rocket_dyn_templates::{context, Template};
use serde::Serialize;
use std::{env, fs};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::error::{AppError, AppResult, ApiError, to_api_error};

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

fn list_videos() -> Vec<VideoItem> {
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
fn index() -> Template {
    let videos = list_videos();
    Template::render("index", context! { videos })
}

#[get("/upload")]
fn upload_page() -> Template {
    Template::render("upload", context! {})
}

#[post("/upload", data = "<form>")]
async fn upload_post(
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

    // Spawn background processing so the request returns immediately.
    let upload_path_bg = upload_path.clone();
    let out_dir_bg = out_dir.clone();
    let id_bg = id.clone();
    task::spawn(async move {
        let thumb_result = generate_thumbnail(&upload_path_bg, &out_dir_bg).await;
        if let Err(e) = run_ffmpeg_to_hls(&upload_path_bg, &out_dir_bg).await {
            let _ = write_processing_log(&id_bg, "ERROR", &e.user_message(), &e.detail_message());
        } else {
            // Clean up original upload after successful packaging to save space.
            let _ = fs::remove_file(&upload_path_bg);
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
fn watch(id: &str) -> Template {
    let id_owned = id.to_string();
    let out_dir = hls_dir().join(id);
    let master = out_dir.join("master.m3u8");
    let error = read_processing_error(id);

    let ready = master.exists();

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
        .register("/", catchers![not_found, payload_too_large, server_error])
}
