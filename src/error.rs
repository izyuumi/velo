use rocket::http::Status;
use rocket::serde::json::Json;
use serde::Serialize;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("storage error while {action}")]
    Storage {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("upload failed: {message}")]
    Upload { message: String },
    #[error("ffmpeg failed: {message}")]
    Ffmpeg { message: String },
}

#[derive(Serialize)]
pub struct ApiError {
    pub message: String,
}

impl AppError {
    pub fn api_status(&self) -> Status {
        match self {
            AppError::Upload { .. } => Status::BadRequest,
            AppError::Storage { .. } | AppError::Ffmpeg { .. } => Status::InternalServerError,
        }
    }

    pub fn user_message(&self) -> String {
        match self {
            AppError::Upload { message } => message.clone(),
            AppError::Storage { .. } => "Server error preparing storage. Please try again.".to_string(),
            AppError::Ffmpeg { .. } => {
                "Processing failed. Ensure ffmpeg is installed and the video format is supported."
                    .to_string()
            }
        }
    }

    pub fn detail_message(&self) -> String {
        match self {
            AppError::Storage { action, source } => {
                format!("storage error while {action}: {source}")
            }
            AppError::Upload { message } | AppError::Ffmpeg { message } => message.clone(),
        }
    }
}

pub fn to_api_error(err: AppError) -> (Status, Json<ApiError>) {
    (
        err.api_status(),
        Json(ApiError {
            message: err.user_message(),
        }),
    )
}
