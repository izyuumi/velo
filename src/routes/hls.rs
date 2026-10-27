use crate::config::hls_dir;
use rocket::fs::NamedFile;
use rocket::http::{ContentType, Header};
use rocket::request::Request;
use rocket::response::{self, Responder};
use std::path::PathBuf;

pub struct CachedFile {
    file: NamedFile,
    cache_control: &'static str,
    content_type: Option<ContentType>,
}

impl CachedFile {
    fn new(file: NamedFile, cache_control: &'static str, content_type: Option<ContentType>) -> Self {
        Self {
            file,
            cache_control,
            content_type,
        }
    }
}

impl<'r> Responder<'r, 'static> for CachedFile {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        let mut response = self.file.respond_to(req)?;
        response.set_header(Header::new("Cache-Control", self.cache_control));
        if let Some(ct) = self.content_type {
            response.set_header(ct);
        }
        Ok(response)
    }
}

fn content_type_for_ext(ext: Option<&str>) -> Option<ContentType> {
    match ext {
        Some("m3u8") => Some(ContentType::new("application", "vnd.apple.mpegurl")),
        Some("ts") => Some(ContentType::new("video", "mp2t")),
        Some("jpg") | Some("jpeg") => Some(ContentType::JPEG),
        _ => None,
    }
}

#[get("/hls/<id>/<path..>")]
pub async fn serve_hls(id: &str, path: PathBuf) -> Option<CachedFile> {
    let base = hls_dir().join(id);
    let full_path = base.join(&path);

    if !full_path.starts_with(&base) {
        return None;
    }

    let file = NamedFile::open(&full_path).await.ok()?;
    let ext = path.extension().and_then(|e| e.to_str());
    let content_type = content_type_for_ext(ext);

    let cache_control = match ext {
        Some("ts") | Some("jpg") | Some("jpeg") => "public, max-age=31536000, immutable",
        Some("m3u8") => "public, max-age=5",
        _ => "public, max-age=3600",
    };

    Some(CachedFile::new(file, cache_control, content_type))
}
