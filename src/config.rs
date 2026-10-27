use std::env;
use std::path::PathBuf;

pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_READY: &str = "ready";
pub const STATUS_ERROR: &str = "error";
pub const MAX_LIST_VIDEOS: i64 = 200;
pub const PROCESSING_LOCK_STALE_SECS: u64 = 6 * 60 * 60;
pub const UPLOAD_SCAN_INTERVAL_SECS: u64 = 10;
pub const PROCESSING_DIR_SUFFIX: &str = ".processing";
pub const DEFAULT_MAX_RETRIES: i64 = 2;
pub const DEFAULT_RETRY_DELAY_SECS: u64 = 30;
pub const PROGRESS_UPDATE_INTERVAL_MS: u64 = 500;
pub const DEFAULT_MAX_WORKERS: usize = 2;

pub fn max_workers() -> usize {
    env::var("FFMPEG_MAX_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_WORKERS)
}

#[derive(Clone)]
pub struct HlsVariant {
    pub width: i64,
    pub bitrate_k: i64,
}

#[derive(Clone)]
pub struct HlsSettings {
    pub segment_seconds: u32,
    pub crf: u8,
    pub preset: String,
    pub audio_bitrate_k: i64,
    pub variants: Vec<HlsVariant>,
}

#[derive(Clone, Copy)]
pub struct RetrySettings {
    pub max_retries: i64,
    pub delay_secs: u64,
}

pub fn data_root() -> PathBuf {
    env::var("DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"))
}

pub fn upload_dir() -> PathBuf {
    data_root().join("uploads")
}

pub fn hls_dir() -> PathBuf {
    data_root().join("hls")
}

pub fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

pub fn locks_dir() -> PathBuf {
    data_root().join("locks")
}

pub fn final_hls_dir(id: &str) -> PathBuf {
    hls_dir().join(id)
}

pub fn processing_hls_dir(id: &str) -> PathBuf {
    hls_dir().join(format!("{id}{PROCESSING_DIR_SUFFIX}"))
}

pub fn sqlite_path() -> PathBuf {
    env::var("SQLITE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_root().join("app.db"))
}

pub fn frontend_dist_dir() -> PathBuf {
    env::var("FRONTEND_DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"))
}

fn parse_variants(value: &str) -> Vec<HlsVariant> {
    value
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.split(':');
            let width = parts.next()?.trim().parse::<i64>().ok()?;
            let bitrate_k = parts.next()?.trim().parse::<i64>().ok()?;
            Some(HlsVariant { width, bitrate_k })
        })
        .collect()
}

pub fn hls_settings() -> HlsSettings {
    let segment_seconds = env::var("HLS_SEGMENT_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(4);
    let crf = env::var("HLS_CRF")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(23);
    let preset = env::var("HLS_PRESET").unwrap_or_else(|_| "veryfast".to_string());
    let audio_bitrate_k = env::var("HLS_AUDIO_BITRATE_K")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(128);
    let variants = env::var("HLS_VARIANTS")
        .ok()
        .map(|v| parse_variants(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            vec![
                HlsVariant { width: 1920, bitrate_k: 5000 },
                HlsVariant { width: 1280, bitrate_k: 2800 },
                HlsVariant { width: 854, bitrate_k: 1400 },
            ]
        });

    HlsSettings {
        segment_seconds,
        crf,
        preset,
        audio_bitrate_k,
        variants,
    }
}

pub fn retry_settings() -> RetrySettings {
    let max_retries = env::var("PROCESSING_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_MAX_RETRIES);
    let delay_secs = env::var("PROCESSING_RETRY_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_RETRY_DELAY_SECS);
    RetrySettings {
        max_retries,
        delay_secs,
    }
}
