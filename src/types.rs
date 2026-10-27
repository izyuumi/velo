use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct VideoItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub progress: Option<f64>,
    pub error: Option<String>,
    pub thumbnail: Option<String>,
    pub duration_seconds: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: i64,
    pub series_id: Option<String>,
    pub episode_number: Option<i64>,
    pub watch_position: Option<i64>,
    pub watch_completed: bool,
}

#[derive(Serialize)]
pub struct UploadResult {
    pub id: String,
    pub title: String,
    pub watch_url: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub id: String,
    pub status: String,
    pub progress: Option<f64>,
    pub error: Option<String>,
    pub title: String,
    pub watch_url: String,
    pub duration_seconds: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: i64,
    pub retries: i64,
    pub upload_available: bool,
    pub series_id: Option<String>,
    pub series_title: Option<String>,
    pub episode_number: Option<i64>,
    pub next_episode_id: Option<String>,
    pub prev_episode_id: Option<String>,
    pub watch_position: Option<i64>,
    pub watch_completed: bool,
}

#[derive(Deserialize)]
pub struct RenameRequest {
    pub title: String,
}

#[derive(Serialize, Clone)]
pub struct SeriesItem {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub cover_video_id: Option<String>,
    pub cover_thumbnail: Option<String>,
    pub episode_count: i64,
    pub total_duration: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub watched_count: i64,
    pub in_progress_count: i64,
}

#[derive(Serialize)]
pub struct SeriesDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub cover_video_id: Option<String>,
    pub episodes: Vec<VideoItem>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Deserialize)]
pub struct CreateSeriesRequest {
    pub title: String,
    pub description: Option<String>,
    pub video_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct UpdateSeriesRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub cover_video_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AddVideosToSeriesRequest {
    pub video_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct ReorderEpisodesRequest {
    pub video_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateProgressRequest {
    pub position_seconds: i64,
    pub completed: Option<bool>,
}

#[derive(Serialize)]
pub struct WatchProgressResponse {
    pub video_id: String,
    pub position_seconds: i64,
    pub completed: bool,
}

#[derive(Deserialize)]
pub struct BulkDeleteRequest {
    pub video_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct BulkAddToSeriesRequest {
    pub video_ids: Vec<String>,
    pub series_id: String,
}
