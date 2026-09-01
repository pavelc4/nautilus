use crate::provider::{MediaItem, MediaMeta, MediaReader};
use std::borrow::Cow;
use tokio_stream::StreamExt;

pub fn parse_quality(quality: &str, label: &str) -> u32 {
    let score = |s: &str| {
        let clean: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty()
            && let Ok(num) = clean.parse::<u32>()
        {
            return Some(num);
        }
        let lower = s.to_lowercase();
        match () {
            _ if lower.contains("hd") || lower.contains("high") || lower.contains("original") => {
                Some(1080)
            }
            _ if lower.contains("medium") => Some(720),
            _ if lower.contains("sd") || lower.contains("low") => Some(360),
            _ => None,
        }
    };

    score(quality).or_else(|| score(label)).unwrap_or(0)
}

pub fn sanitize_filename(title: &str, default: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim_matches('_');
    if clean.is_empty() {
        default.to_string()
    } else {
        clean.to_string()
    }
}

/// Read-ahead buffer for the CDN->Telegram stream-through. Sized to Telegram's
/// 512 KiB upload part so grammers fills a whole part per refill instead of ~4
/// (128 KiB before). Larger gives diminishing returns — the part size caps it.
const STREAM_BUF_BYTES: usize = 512 * 1024;

pub async fn fetch_stream(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<(u64, MediaReader, Option<String>)> {
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("Failed to fetch stream from CDN: {}", resp.status());
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_lowercase());
    let mut size = resp.content_length().unwrap_or(0);
    let stream = resp
        .bytes_stream()
        .map(|r| r.map_err(std::io::Error::other));
    let stream_reader = tokio_util::io::StreamReader::new(stream);

    if size == 0 {
        // its size (Telegram needs total parts up front). Fine for small clips,
        // a memory bomb for a 2 GB video. Upgrade path: HEAD/Range preflight for
        // size, or spill to a temp file, so memory stays flat. See streaming notes.
        use tokio::io::AsyncReadExt;
        let mut buf_reader = tokio::io::BufReader::with_capacity(STREAM_BUF_BYTES, stream_reader);
        let mut buffer = Vec::new();
        buf_reader.read_to_end(&mut buffer).await?;
        size = buffer.len() as u64;
        let cursor = std::io::Cursor::new(buffer);
        let reader: MediaReader = Box::pin(cursor);
        Ok((size, reader, content_type))
    } else {
        let buffered = tokio::io::BufReader::with_capacity(STREAM_BUF_BYTES, stream_reader);
        let reader: MediaReader = Box::pin(buffered);
        Ok((size, reader, content_type))
    }
}

pub fn parse_dimensions(quality: &str, label: &str, is_vertical: bool) -> Option<(i32, i32)> {
    let parse_w_h = |s: &str| {
        let parts: Vec<&str> = s.split(['x', 'X', '*']).collect();
        if parts.len() == 2 {
            let w = parts[0]
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<i32>()
                .ok();
            let h = parts[1]
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<i32>()
                .ok();
            if let (Some(width), Some(height)) = (w, h) {
                return Some((width, height));
            }
        }
        None
    };

    if let Some(dims) = parse_w_h(quality) {
        return Some(dims);
    }
    if let Some(dims) = parse_w_h(label) {
        return Some(dims);
    }

    let extract_single = |s: &str| {
        let clean: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty()
            && let Ok(num) = clean.parse::<i32>()
        {
            match num {
                1080 => {
                    return Some(if is_vertical {
                        (1080, 1920)
                    } else {
                        (1920, 1080)
                    });
                }
                720 => {
                    return Some(if is_vertical {
                        (720, 1280)
                    } else {
                        (1280, 720)
                    });
                }
                480 => return Some(if is_vertical { (480, 854) } else { (854, 480) }),
                360 => return Some(if is_vertical { (360, 640) } else { (640, 360) }),
                240 => return Some(if is_vertical { (240, 426) } else { (426, 240) }),
                144 => return Some(if is_vertical { (144, 256) } else { (256, 144) }),
                _ => {}
            }
        }
        None
    };

    extract_single(quality).or_else(|| extract_single(label))
}

pub async fn download_item(
    client: &reqwest::Client,
    url: String,
    mut meta: MediaMeta,
) -> anyhow::Result<MediaItem> {
    let (size, reader, content_type) = fetch_stream(client, &url).await?;
    meta.size = size;
    // Photos must match the actual on-wire type: a source that returns PNG
    // (e.g. Pixiv originals) but is labelled image/jpeg makes Telegram reject
    // the upload with PHOTO_SAVE_FILE_INVALID.
    if meta.kind == crate::provider::MediaKind::Photo {
        if let Some(ext) = image_ext(content_type.as_deref()) {
            let mime = format!("image/{ext}");
            meta.mime_type = Cow::Owned(mime);
            if let Some(dot) = meta.filename.rfind('.') {
                meta.filename.replace_range(dot.., &format!(".{ext}"));
            }
        }
    }
    Ok(MediaItem { meta, reader })
}

fn image_ext(content_type: Option<&str>) -> Option<&'static str> {
    match content_type {
        Some("image/png") => Some("png"),
        Some("image/jpeg") | Some("image/jpg") => Some("jpg"),
        Some("image/webp") => Some("webp"),
        Some("image/gif") => Some("gif"),
        Some("image/bmp") => Some("bmp"),
        _ => None,
    }
}
