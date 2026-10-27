use crate::config::{HlsSettings, PROGRESS_UPDATE_INTERVAL_MS};
use crate::db::update_video_progress;
use crate::error::{AppError, AppResult};
use rocket::tokio::process::Command;
use serde_json::Value;
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Stdio;
use std::{env, fs};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::task;
use tokio::time::Instant;

pub struct MediaMetadata {
    pub duration_seconds: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub has_audio: bool,
}

fn progress_percent(out_time_us: f64, duration_seconds: i64) -> f64 {
    let total_us = duration_seconds.saturating_mul(1_000_000) as f64;
    ((out_time_us / total_us) * 100.0).clamp(0.0, 99.9)
}

pub async fn probe_video_metadata(input_mp4: &Path) -> AppResult<MediaMetadata> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        return Ok(MediaMetadata {
            duration_seconds: None,
            width: None,
            height: None,
            has_audio: true,
        });
    }

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("stream=width,height,codec_type")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("json")
        .arg(input_mp4)
        .output()
        .await
        .map_err(|e| AppError::Ffmpeg {
            message: format!("ffprobe spawn failed: {e}"),
        })?;

    if !output.status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("ffprobe exited with status: {}", output.status),
        });
    }

    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| AppError::Ffmpeg {
        message: format!("ffprobe output parse failed: {e}"),
    })?;

    let duration_seconds = value
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|d| d.round() as i64);

    let streams = value.get("streams").and_then(|s| s.as_array());

    let (width, height) = streams
        .and_then(|arr| {
            arr.iter().find(|s| {
                s.get("codec_type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "video")
                    .unwrap_or(false)
            })
        })
        .map(|stream| {
            let width = stream.get("width").and_then(|v| v.as_i64());
            let height = stream.get("height").and_then(|v| v.as_i64());
            (width, height)
        })
        .unwrap_or((None, None));

    let has_audio = streams
        .map(|arr| {
            arr.iter().any(|stream| {
                stream
                    .get("codec_type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "audio")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    Ok(MediaMetadata {
        duration_seconds,
        width,
        height,
        has_audio,
    })
}

pub async fn run_ffmpeg_to_hls(
    input_mp4: &Path,
    out_dir: &Path,
    settings: &HlsSettings,
    metadata: &MediaMetadata,
    db: &SqlitePool,
    id: &str,
) -> AppResult<()> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        let master = out_dir.join("master.m3u8");
        let _ = fs::write(&master, "#EXTM3U\n#EXT-X-VERSION:3\n");
        update_video_progress(db, id, 100.0).await?;
        return Ok(());
    }

    let master = "master.m3u8";
    let segment_pattern = "v%v/seg_%03d.ts";
    let playlist_pattern = "v%v/prog_index.m3u8";

    for (idx, _variant) in settings.variants.iter().enumerate() {
        let variant_dir = out_dir.join(format!("v{idx}"));
        let _ = fs::create_dir_all(&variant_dir);
    }

    let input_abs = fs::canonicalize(input_mp4).map_err(|e| AppError::Storage {
        action: "canonicalizing upload path",
        source: e,
    })?;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-nostats")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-i")
        .arg(input_abs)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(out_dir);

    let mut filter = String::new();
    if settings.variants.len() > 1 {
        filter.push_str(&format!("[0:v]split={}", settings.variants.len()));
        for idx in 0..settings.variants.len() {
            filter.push_str(&format!("[v{idx}]"));
        }
        filter.push(';');
        for (idx, variant) in settings.variants.iter().enumerate() {
            filter.push_str(&format!(
                "[v{idx}]scale={}:-2[v{idx}out];",
                variant.width
            ));
        }
        filter.pop();
        cmd.arg("-filter_complex").arg(filter);
        for idx in 0..settings.variants.len() {
            cmd.arg("-map").arg(format!("[v{idx}out]"));
            if metadata.has_audio {
                cmd.arg("-map").arg("0:a:0");
            }
        }
    } else if let Some(variant) = settings.variants.first() {
        cmd.arg("-vf").arg(format!("scale={}:-2", variant.width));
        cmd.arg("-map").arg("0:v:0");
        if metadata.has_audio {
            cmd.arg("-map").arg("0:a:0");
        }
    }

    cmd.arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg(&settings.preset)
        .arg("-crf")
        .arg(settings.crf.to_string())
        .arg("-force_key_frames")
        .arg(format!(
            "expr:gte(t,n_forced*{})",
            settings.segment_seconds
        ))
        .arg("-sc_threshold")
        .arg("0")
        .arg("-profile:v")
        .arg("main")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", settings.audio_bitrate_k));

    for (idx, variant) in settings.variants.iter().enumerate() {
        cmd.arg(format!("-b:v:{idx}"))
            .arg(format!("{}k", variant.bitrate_k));
    }

    let var_map: String = if metadata.has_audio {
        (0..settings.variants.len())
            .map(|idx| format!("v:{idx},a:{idx}"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        (0..settings.variants.len())
            .map(|idx| format!("v:{idx}"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    cmd.arg("-f")
        .arg("hls")
        .arg("-hls_time")
        .arg(settings.segment_seconds.to_string())
        .arg("-hls_playlist_type")
        .arg("vod")
        .arg("-hls_flags")
        .arg("independent_segments")
        .arg("-hls_segment_filename")
        .arg(segment_pattern)
        .arg("-master_pl_name")
        .arg(master)
        .arg("-var_stream_map")
        .arg(var_map)
        .arg(playlist_pattern);

    let mut child = cmd.spawn().map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg spawn failed: {e}"),
    })?;

    let stdout = child.stdout.take().ok_or_else(|| AppError::Ffmpeg {
        message: "ffmpeg stdout missing".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| AppError::Ffmpeg {
        message: "ffmpeg stderr missing".to_string(),
    })?;
    let stderr_task = task::spawn(async move {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(stderr);
        reader
            .read_to_end(&mut buf)
            .await
            .map(|_| String::from_utf8_lossy(&buf).to_string())
    });
    let mut reader = BufReader::new(stdout).lines();
    let mut last_update = Instant::now();
    while let Some(line) = reader.next_line().await.map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg progress read failed: {e}"),
    })? {
        if let Some(value) = line.strip_prefix("out_time_ms=") {
            if let Ok(out_ms) = value.trim().parse::<f64>() {
                if let Some(duration) = metadata.duration_seconds {
                    let pct = progress_percent(out_ms, duration);
                    if last_update.elapsed().as_millis() as u64 >= PROGRESS_UPDATE_INTERVAL_MS {
                        let _ = update_video_progress(db, id, pct).await;
                        last_update = Instant::now();
                    }
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| AppError::Ffmpeg {
        message: format!("ffmpeg wait failed: {e}"),
    })?;
    let stderr_output = stderr_task.await.unwrap_or_else(|_| Ok(String::new())).unwrap_or_default();
    if !status.success() {
        return Err(AppError::Ffmpeg {
            message: format!("ffmpeg exited with status: {status}. {stderr_output}"),
        });
    }

    update_video_progress(db, id, 100.0).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::progress_percent;

    #[test]
    fn ffmpeg_microsecond_progress_is_converted_to_percent() {
        assert_eq!(progress_percent(30_000_000.0, 60), 50.0);
    }
}

pub async fn generate_thumbnail(input_mp4: &Path, out_dir: &Path) -> AppResult<()> {
    if env::var("FFMPEG_DRY_RUN").ok().as_deref() == Some("1") {
        let thumb_path = out_dir.join("thumb.jpg");
        let _ = fs::write(thumb_path, []);
        return Ok(());
    }

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
