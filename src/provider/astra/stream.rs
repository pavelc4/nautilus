use std::borrow::Cow;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;

use crate::provider::{MediaItem, MediaMeta, MediaReader};

/// Read-ahead buffer for the CDN->Telegram stream-through. Sized to Telegram's
/// 512 KiB upload part so grammers fills a whole part per refill instead of ~4
/// (128 KiB before). Larger gives diminishing returns — the part size caps it.
pub const STREAM_BUF_BYTES: usize = 512 * 1024;

pub async fn fetch_stream(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<(u64, MediaReader, Option<String>, Option<(i32, i32)>)> {
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
    let mut stream_reader = tokio_util::io::StreamReader::new(stream);

    let mut peek_buf = vec![0u8; 2048];
    let mut n = 0;
    while n < peek_buf.len() {
        match stream_reader.read(&mut peek_buf[n..]).await {
            Ok(0) => break,
            Ok(bytes_read) => n += bytes_read,
            Err(e) => return Err(e.into()),
        }
    }
    peek_buf.truncate(n);
    let dims = detect_image_dimensions(&peek_buf);

    if size == 0 {
        // Upstream omitted Content-Length. Spill to an unlinked temporary disk file so RAM
        // usage stays flat O(1) instead of blowing up memory for multi-gigabyte media.
        let temp_dir = std::env::temp_dir();
        let temp_filename = format!(
            "nautilus_stream_{}_{}.tmp",
            std::process::id(),
            rand::random::<u64>()
        );
        let temp_path = temp_dir.join(temp_filename);

        let mut file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)
            .await?;

        // Write peek_buf first
        tokio::io::AsyncWriteExt::write_all(&mut file, &peek_buf).await?;

        // Copy remaining stream chunks to file
        let mut buf_reader = tokio::io::BufReader::with_capacity(STREAM_BUF_BYTES, stream_reader);
        tokio::io::copy(&mut buf_reader, &mut file).await?;

        // Sync and rewind
        tokio::io::AsyncWriteExt::flush(&mut file).await?;
        size = file.metadata().await?.len();
        tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(0)).await?;

        // Unlink on disk; open file handle remains valid on Linux until dropped
        let _ = tokio::fs::remove_file(&temp_path).await;

        let buffered = tokio::io::BufReader::with_capacity(STREAM_BUF_BYTES, file);
        let reader: MediaReader = Box::pin(buffered);
        Ok((size, reader, content_type, dims))
    } else {
        let cursor = std::io::Cursor::new(peek_buf);
        let chained = cursor.chain(stream_reader);
        let buffered = tokio::io::BufReader::with_capacity(STREAM_BUF_BYTES, chained);
        let reader: MediaReader = Box::pin(buffered);
        Ok((size, reader, content_type, dims))
    }
}

pub async fn download_item(
    client: &reqwest::Client,
    url: String,
    mut meta: MediaMeta,
) -> anyhow::Result<MediaItem> {
    let (size, mut reader, content_type, dims) = fetch_stream(client, &url).await?;
    meta.size = size;
    if meta.dims.is_none() {
        meta.dims = dims;
    }

    if meta.kind == crate::provider::MediaKind::Photo {
        // Convert WebP images to JPEG so Telegram accepts them as photos
        // instead of rejecting with PHOTO_EXT_INVALID or sending as stickers.
        let is_webp = content_type.as_deref() == Some("image/webp")
            || meta.mime_type == "image/webp"
            || meta.filename.ends_with(".webp");

        if is_webp {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::with_capacity(meta.size.max(1024) as usize);
            reader.read_to_end(&mut buf).await?;
            if let Ok(img) = image::load_from_memory_with_format(&buf, image::ImageFormat::WebP) {
                let mut jpeg_buf = Vec::new();
                let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
                if img.write_to(&mut cursor, image::ImageFormat::Jpeg).is_ok() {
                    meta.size = jpeg_buf.len() as u64;
                    meta.mime_type = Cow::Borrowed("image/jpeg");
                    if let Some(dot) = meta.filename.rfind('.') {
                        meta.filename.replace_range(dot.., ".jpg");
                    }
                    reader = Box::pin(std::io::Cursor::new(jpeg_buf));
                } else {
                    reader = Box::pin(std::io::Cursor::new(buf));
                }
            } else {
                reader = Box::pin(std::io::Cursor::new(buf));
            }
        } else if let Some(ext) = image_ext(content_type.as_deref()) {
            // Photos must match the actual on-wire type: a source that returns PNG
            // (e.g. Pixiv originals) but is labelled image/jpeg makes Telegram reject
            // the upload with PHOTO_SAVE_FILE_INVALID.
            let mime = format!("image/{ext}");
            meta.mime_type = Cow::Owned(mime);
            if let Some(dot) = meta.filename.rfind('.') {
                meta.filename.replace_range(dot.., &format!(".{ext}"));
            }
        }
    }
    Ok(MediaItem { meta, reader })
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
            if let (Some(w), Some(h)) = (w, h)
                && w > 0
                && h > 0
            {
                return Some((w, h));
            }
        }
        None
    };

    if let Some(dims) = parse_w_h(quality).or_else(|| parse_w_h(label)) {
        return Some(dims);
    }

    let extract_single = |s: &str| {
        let clean: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty()
            && let Ok(val) = clean.parse::<i32>()
        {
            return match val {
                2160 => Some(if is_vertical { (2160, 3840) } else { (3840, 2160) }),
                1440 => Some(if is_vertical { (1440, 2560) } else { (2560, 1440) }),
                1080 => Some(if is_vertical { (1080, 1920) } else { (1920, 1080) }),
                720 => Some(if is_vertical { (720, 1280) } else { (1280, 720) }),
                480 => Some(if is_vertical { (480, 854) } else { (854, 480) }),
                360 => Some(if is_vertical { (360, 640) } else { (640, 360) }),
                240 => Some(if is_vertical { (240, 426) } else { (426, 240) }),
                144 => Some(if is_vertical { (144, 256) } else { (256, 144) }),
                _ => None,
            };
        }
        None
    };

    extract_single(quality).or_else(|| extract_single(label))
}

/// Parse image dimensions directly from file headers (JPEG, PNG, GIF, WebP).
pub fn detect_image_dimensions(data: &[u8]) -> Option<(i32, i32)> {
    if data.len() < 10 {
        return None;
    }

    // 1. PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) && data.len() >= 24 {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as i32;
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as i32;
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }

    // 2. GIF: GIF87a or GIF89a
    if (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) && data.len() >= 10 {
        let w = u16::from_le_bytes([data[6], data[7]]) as i32;
        let h = u16::from_le_bytes([data[8], data[9]]) as i32;
        if w > 0 && h > 0 {
            return Some((w, h));
        }
    }

    // 3. WebP: RIFF .... WEBP
    if data.starts_with(b"RIFF") && data.len() >= 30 && &data[8..12] == b"WEBP" {
        if &data[12..16] == b"VP8 " && data.len() >= 30 {
            let w = (((data[26] as u16) | ((data[27] as u16) << 8)) & 0x3FFF) as i32;
            let h = (((data[28] as u16) | ((data[29] as u16) << 8)) & 0x3FFF) as i32;
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        } else if &data[12..16] == b"VP8L" && data.len() >= 25 {
            let b0 = data[21] as u32;
            let b1 = data[22] as u32;
            let b2 = data[23] as u32;
            let b3 = data[24] as u32;
            let w = 1 + ((b0 | ((b1 & 0x3F) << 8)) as i32);
            let h = 1 + (((b1 >> 6) | (b2 << 2) | ((b3 & 0xF) << 10)) as i32);
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        } else if &data[12..16] == b"VP8X" && data.len() >= 30 {
            let w = 1 + ((data[24] as i32) | ((data[25] as i32) << 8) | ((data[26] as i32) << 16));
            let h = 1 + ((data[27] as i32) | ((data[28] as i32) << 8) | ((data[29] as i32) << 16));
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
    }

    // 4. JPEG: FF D8 ...
    if data.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 8 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if (0xC0..=0xC3).contains(&marker)
                || (0xC5..=0xC7).contains(&marker)
                || (0xC9..=0xCB).contains(&marker)
                || (0xCD..=0xCF).contains(&marker)
            {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as i32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as i32;
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
            if marker != 0xD8 && marker != 0xD9 && marker != 0x00 && marker != 0xFF {
                if i + 3 >= data.len() {
                    break;
                }
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                i += 2 + len;
            } else {
                i += 1;
            }
        }
    }

    None
}

pub fn image_ext(content_type: Option<&str>) -> Option<&'static str> {
    match content_type {
        Some("image/png") => Some("png"),
        Some("image/jpeg") | Some("image/jpg") => Some("jpg"),
        Some("image/webp") => Some("webp"),
        Some("image/gif") => Some("gif"),
        Some("image/bmp") => Some("bmp"),
        _ => None,
    }
}
