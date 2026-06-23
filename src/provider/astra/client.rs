use crate::provider::{MediaItem, MediaMeta, MediaReader};
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

pub async fn fetch_stream(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<(u64, MediaReader)> {
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("Failed to fetch stream from CDN: {}", resp.status());
    }
    let mut size = resp.content_length().unwrap_or(0);
    let stream = resp
        .bytes_stream()
        .map(|r| r.map_err(std::io::Error::other));
    let stream_reader = tokio_util::io::StreamReader::new(stream);

    if size == 0 {
        use tokio::io::AsyncReadExt;
        let mut buf_reader = tokio::io::BufReader::new(stream_reader);
        let mut buffer = Vec::new();
        buf_reader.read_to_end(&mut buffer).await?;
        size = buffer.len() as u64;
        let cursor = std::io::Cursor::new(buffer);
        let reader: MediaReader = Box::pin(cursor);
        Ok((size, reader))
    } else {
        let buffered = tokio::io::BufReader::with_capacity(128 * 1024, stream_reader);
        let reader: MediaReader = Box::pin(buffered);
        Ok((size, reader))
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
    let (size, reader) = fetch_stream(client, &url).await?;
    meta.size = size;
    Ok(MediaItem { meta, reader })
}
