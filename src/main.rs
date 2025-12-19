#[macro_use]
extern crate rocket;

use rocket::form::Form;
use rocket::fs::{FileServer, TempFile};
use rocket::http::Status;
use rocket::response::Redirect;
use rocket::tokio::process::Command;
use rocket::tokio::task;
use rocket_dyn_templates::{context, Template};
use serde::Serialize;
use std::{env, fs};
use std::path::{Path, PathBuf};
use uuid::Uuid;

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

#[derive(FromForm)]
struct UploadForm<'r> {
    video: TempFile<'r>,
    title: Option<&'r str>,
}

#[derive(Serialize)]
struct VideoItem {
    id: String,
    title: String,
}

#[derive(Serialize)]
struct WatchContext {
    id: String,
    ready: bool,
    error: Option<String>,
}

fn ensure_dirs() -> std::io::Result<()> {
    fs::create_dir_all(upload_dir())?;
    fs::create_dir_all(hls_dir())?;
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

        if dir_path.join("master.m3u8").exists() {
            items.push(VideoItem { id, title });
        }
    }

    items.sort_by(|a, b| b.id.cmp(&a.id));
    items
}

async fn run_ffmpeg_to_hls(input_mp4: &Path, out_dir: &Path) -> Result<(), String> {
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
        .map_err(|e| format!("ffmpeg spawn failed: {e}"))?;

    if !status.success() {
        return Err(format!("ffmpeg exited with status: {status}"));
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
async fn upload_post(mut form: Form<UploadForm<'_>>) -> Result<Redirect, (Status, String)> {
    ensure_dirs().map_err(|e| (Status::InternalServerError, e.to_string()))?;

    let id = Uuid::new_v4().to_string();
    let title = form.title.unwrap_or("").trim().to_string();

    let upload_path = upload_dir().join(format!("{id}.mp4"));
    let out_dir = hls_dir().join(&id);

    fs::create_dir_all(&out_dir).map_err(|e| (Status::InternalServerError, e.to_string()))?;

    form.video
        .persist_to(&upload_path)
        .await
        .map_err(|e| (Status::InternalServerError, format!("save upload failed: {e}")))?;

    if !title.is_empty() {
        let _ = fs::write(out_dir.join("title.txt"), title);
    }

    // Spawn background processing so the request returns immediately.
    let upload_path_bg = upload_path.clone();
    let out_dir_bg = out_dir.clone();
    task::spawn(async move {
        if let Err(e) = run_ffmpeg_to_hls(&upload_path_bg, &out_dir_bg).await {
            let _ = fs::write(out_dir_bg.join("error.txt"), e);
        } else {
            // Clean up original upload after successful packaging to save space.
            let _ = fs::remove_file(&upload_path_bg);
        }
    });

    Ok(Redirect::to(uri!(watch(id = id))))
}

#[get("/watch/<id>")]
fn watch(id: String) -> Template {
    let out_dir = hls_dir().join(&id);
    let master = out_dir.join("master.m3u8");
    let error_path = out_dir.join("error.txt");

    let ready = master.exists();
    let error = fs::read_to_string(&error_path).ok();

    let ctx = WatchContext { id: id.clone(), ready, error };
    Template::render("watch", context! { ctx })
}

#[launch]
fn rocket() -> _ {
    ensure_dirs().ok();

    let hls_path = hls_dir();

    rocket::build()
        .mount("/", routes![index, upload_page, upload_post, watch])
        .mount("/hls", FileServer::from(hls_path))
        .attach(Template::fairing())
}
