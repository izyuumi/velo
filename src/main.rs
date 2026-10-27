#[macro_use]
extern crate rocket;

mod config;
mod db;
mod error;
mod processing;
mod routes;
mod types;

use config::{
    final_hls_dir, frontend_dist_dir, hls_settings, processing_hls_dir, retry_settings,
    upload_dir, HlsSettings, RetrySettings, UPLOAD_SCAN_INTERVAL_SECS,
};
use db::{
    add_videos_to_series, bulk_delete_videos, create_series, delete_series, delete_video_record,
    ensure_dirs, get_continue_watching, get_series_detail, get_watch_progress, init_db,
    insert_video, list_series, list_videos, read_video_status_full, remove_video_from_series,
    reorder_series_episodes, reset_for_reprocess, update_series_info, update_video_title,
    update_watch_progress,
};
use error::{to_api_error, ApiError};
use processing::{
    reconcile_processing_jobs, scan_uploads_for_jobs, spawn_processing_task,
    spawn_reprocessing_task,
};
use routes::serve_hls;
use types::{
    AddVideosToSeriesRequest, BulkAddToSeriesRequest, BulkDeleteRequest, CreateSeriesRequest,
    RenameRequest, ReorderEpisodesRequest, SeriesDetail, SeriesItem, StatusResponse,
    UpdateProgressRequest, UpdateSeriesRequest, UploadResult, VideoItem, WatchProgressResponse,
};

use rocket::fairing::AdHoc;
use rocket::form::Form;
use rocket::fs::{NamedFile, TempFile};
use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use rocket::tokio::process::Command;
use rocket::tokio::task;
use rocket::Request;
use rocket::State;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use std::{env, fs};
use uuid::Uuid;

struct AppState {
    db: SqlitePool,
    hls: HlsSettings,
    retry: RetrySettings,
}

#[derive(FromForm)]
struct UploadForm<'r> {
    video: TempFile<'r>,
    title: Option<&'r str>,
}

#[derive(FromForm)]
struct VideosQuery<'r> {
    search: Option<&'r str>,
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

#[get("/api/videos?<query..>")]
async fn list_videos_api(state: &State<AppState>, query: VideosQuery<'_>) -> Json<Vec<VideoItem>> {
    let videos = match list_videos(&state.db, query.search).await {
        Ok(videos) => videos,
        Err(_) => Vec::new(),
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
async fn rename_video_api(
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
async fn reprocess_video_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<StatusResponse>, (Status, Json<ApiError>)> {
    let upload_path = upload_dir().join(format!("{id}.mp4"));
    if !upload_path.exists() {
        return Err((Status::BadRequest, Json(ApiError { message: "Original upload missing; cannot reprocess.".to_string() })));
    }

    reset_for_reprocess(&state.db, id).await.map_err(to_api_error)?;
    spawn_reprocessing_task(id.to_string(), state.db.clone(), state.hls.clone(), state.retry);

    let status = read_video_status_full(&state.db, id)
        .await
        .map_err(to_api_error)?
        .ok_or_else(|| (Status::NotFound, Json(ApiError { message: "Video not found.".to_string() })))?;
    Ok(Json(status))
}

#[delete("/api/video/<id>")]
async fn delete_video_api(
    state: &State<AppState>,
    id: &str,
) -> Result<Json<serde_json::Value>, (Status, Json<ApiError>)> {
    let final_dir = final_hls_dir(id);
    let processing_dir = processing_hls_dir(id);
    let upload_path = upload_dir().join(format!("{id}.mp4"));

    let _ = fs::remove_dir_all(final_dir);
    let _ = fs::remove_dir_all(processing_dir);
    let _ = fs::remove_file(upload_path);

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
        to_api_error(error::AppError::Storage {
            action: "creating output directory",
            source: e,
        })
    })?;

    if let Err(e) = form.video.persist_to(&upload_path).await {
        if e.raw_os_error() == Some(18) {
            form.video
                .copy_to(&upload_path)
                .await
                .map_err(|e| {
                    to_api_error(error::AppError::Upload {
                        message: format!("Saving the upload failed: {e}"),
                    })
                })?;
        } else {
            return Err(to_api_error(error::AppError::Upload {
                message: format!("Saving the upload failed: {e}"),
            }));
        }
    }

    let title_for_db = if title.trim().is_empty() { id.clone() } else { title.clone() };
    insert_video(&state.db, &id, &title_for_db).await.map_err(to_api_error)?;

    spawn_processing_task(id.clone(), state.db.clone(), state.hls.clone(), state.retry);

    Ok(Json(UploadResult {
        id: id.clone(),
        title,
        watch_url: format!("/watch/{id}"),
    }))
}

#[get("/")]
async fn index_page() -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/sw.js")]
async fn service_worker() -> Option<(ContentType, NamedFile)> {
    let file = NamedFile::open(frontend_dist_dir().join("sw.js")).await.ok()?;
    Some((ContentType::JavaScript, file))
}

#[get("/upload")]
async fn upload_page() -> Option<NamedFile> {
    NamedFile::open(frontend_dist_dir().join("index.html")).await.ok()
}

#[get("/downloaded")]
async fn downloaded_page() -> Option<NamedFile> {
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
    let _ = ensure_dirs();

    let frontend_assets = frontend_dist_dir().join("assets");

    rocket::build()
        .mount(
            "/",
            routes![
                index_page,
                service_worker,
                upload_page,
                downloaded_page,
                upload_post,
                watch_page,
                series_page,
                series_manage_page,
                list_videos_api,
                continue_watching_api,
                status_api,
                rename_video_api,
                reprocess_video_api,
                delete_video_api,
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
                hls_js_map,
                serve_hls
            ],
        )
        .mount("/assets", rocket::fs::FileServer::from(frontend_assets))
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
                if env::var("ROCKET_PROFILE").ok().as_deref() == Some("release") {
                    return;
                }

                let frontend_dir = std::path::PathBuf::from("frontend");

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
            .header(
                ContentType::new("multipart", "form-data")
                    .with_params([("boundary", boundary)]),
            )
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
