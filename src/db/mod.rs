mod migrations;
mod progress;
mod series;
mod videos;

pub use migrations::*;
pub use progress::*;
pub use series::*;
pub use videos::*;

use crate::config::{data_root, hls_dir, locks_dir, logs_dir, upload_dir};
use crate::error::{AppError, AppResult};
use std::fs;
use std::path::Path;

pub fn ensure_dirs() -> AppResult<()> {
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
    fs::create_dir_all(locks_dir()).map_err(|e| AppError::Storage {
        action: "creating locks directory",
        source: e,
    })?;
    Ok(())
}
