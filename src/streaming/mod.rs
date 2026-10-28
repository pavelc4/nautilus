use std::fmt::Write as _;
use std::sync::atomic::Ordering;
use std::time::Duration;

use grammers_client::Client;
use grammers_client::media::{Attribute, InputMedia, Uploaded};
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::config::Config;
use crate::provider::{MediaKind, MediaMeta, MediaReader};
use crate::streaming::progress::ProgressReader;

pub mod progress;

/// Telegram rejects inline photos over ~10 MB with PHOTO_SAVE_FILE_INVALID;
/// larger images must be sent as documents.
const PHOTO_MAX_SIZE: u64 = 10 * 1024 * 1024;
const PHOTO_MAX_DIM_SUM: i32 = 10_000;
const PHOTO_MAX_ASPECT_RATIO: f64 = 20.0;

/// Photos over Telegram's inline-photo size ceiling or geometry constraints
/// (width + height <= 10,000, aspect ratio <= 20:1) are sent as documents so
/// they don't fail with PHOTO_INVALID_DIMENSIONS or PHOTO_SAVE_FILE_INVALID.
pub fn send_as_photo(meta: &MediaMeta) -> bool {
    if meta.kind != MediaKind::Photo || meta.size > PHOTO_MAX_SIZE {
        return false;
    }
    if let Some((w, h)) = meta.dims {
        if w <= 0 || h <= 0 || (w + h) > PHOTO_MAX_DIM_SUM {
            return false;
        }
        let ratio = w as f64 / h as f64;
        if ratio > PHOTO_MAX_ASPECT_RATIO || (1.0 / ratio) > PHOTO_MAX_ASPECT_RATIO {
            return false;
        }
    }
    true
}

pub async fn upload_thumbnail(
    client: &Client,
    http_client: &reqwest::Client,
    url: &str,
) -> Option<Uploaded> {
    let resp = http_client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    let len = bytes.len();
    let mut cursor = std::io::Cursor::new(bytes);
    client
        .upload_stream(&mut cursor, len, "thumb.jpg".to_string())
        .await
        .ok()
}

pub async fn upload_media(
    client: &Client,
    chat: PeerRef,
    status_msg_id: i32,
    meta: MediaMeta,
    reader: MediaReader,
    config: &Config,
    http_client: &reqwest::Client,
) -> anyhow::Result<(Uploaded, Option<Uploaded>)> {
    let thumb_uploaded = if meta.kind == MediaKind::Video
        && let Some(ref thumb_url) = meta.thumbnail_url
    {
        upload_thumbnail(client, http_client, thumb_url).await
    } else {
        None
    };

    let (mut progress_reader, byte_counter) = ProgressReader::new(reader);

    let size = meta.size as usize;
    let filename = meta.filename.clone();

    let progress_task = if meta.size > 0 {
        let edit_interval = Duration::from_secs(config.progress_edit_secs());
        let total = meta.size;
        let byte_counter = byte_counter.clone();
        let client = client.clone();

        Some(tokio::spawn(async move {
            let mut last_edited = tokio::time::Instant::now();
            let start_time = tokio::time::Instant::now();
            // Buffers reused across ticks — cleared and rewritten in place each edit
            // instead of allocating ~6 fresh Strings per tick (format! + .repeat()).
            let mut bar = String::with_capacity(10);
            let mut text = String::with_capacity(160);
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let read = byte_counter.load(Ordering::Relaxed);
                if read >= total {
                    let _ = client
                        .edit_message(
                            chat,
                            status_msg_id,
                            InputMessage::new().text("Upload complete, sending..."),
                        )
                        .await;
                    return;
                }
                if last_edited.elapsed() >= edit_interval {
                    let pct = read as f64 / total as f64 * 100.0;
                    let filled = ((pct / 10.0).floor() as usize).clamp(0, 10);
                    bar.clear();
                    for _ in 0..filled {
                        bar.push('#');
                    }
                    for _ in 0..(10 - filled) {
                        bar.push('-');
                    }

                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.0 {
                        read as f64 / elapsed
                    } else {
                        0.0
                    };

                    let read_mb = read as f64 / (1024.0 * 1024.0);
                    let total_mb = total as f64 / (1024.0 * 1024.0);

                    text.clear();
                    let _ = write!(
                        text,
                        "⚡️ <b>Uploading:</b> [<code>{bar}</code>] {pct:.1}%\n\
                         ├ <b>Progress:</b> {read_mb:.2} MB / {total_mb:.2} MB\n\
                         ├ <b>Speed:</b> "
                    );
                    if speed > 1024.0 * 1024.0 {
                        let _ = write!(text, "{:.2} MB/s", speed / (1024.0 * 1024.0));
                    } else if speed > 1024.0 {
                        let _ = write!(text, "{:.1} KB/s", speed / 1024.0);
                    } else {
                        let _ = write!(text, "{:.0} B/s", speed);
                    }
                    text.push_str("\n└ <b>ETA:</b> ");
                    if speed > 0.0 {
                        let remaining_bytes = total.saturating_sub(read) as f64;
                        let eta_secs = (remaining_bytes / speed).round() as u64;
                        if eta_secs > 3600 {
                            let _ = write!(
                                text,
                                "{:02}:{:02}:{:02}",
                                eta_secs / 3600,
                                (eta_secs % 3600) / 60,
                                eta_secs % 60
                            );
                        } else {
                            let _ = write!(text, "{:02}:{:02}", eta_secs / 60, eta_secs % 60);
                        }
                    } else {
                        text.push_str("--:--");
                    }

                    let _ = client
                        .edit_message(chat, status_msg_id, InputMessage::new().html(&text))
                        .await;
                    last_edited = tokio::time::Instant::now();
                }
            }
        }))
    } else {
        None
    };

    let uploaded = client
        .upload_stream(&mut progress_reader, size, filename)
        .await?;

    if let Some(handle) = progress_task {
        handle.abort();
    }

    Ok((uploaded, thumb_uploaded))
}

pub fn build_media_message(
    caption: &str,
    meta: &MediaMeta,
    uploaded: Uploaded,
    thumb: Option<Uploaded>,
) -> InputMessage {
    let mut msg = InputMessage::new().html(caption);
    msg = msg.mime_type(&meta.mime_type);

    if send_as_photo(meta) {
        msg = msg.photo(uploaded);
    } else {
        msg = msg.document(uploaded);
        if let Some(t) = thumb {
            msg = msg.thumbnail(t);
        }
    }

    if let MediaKind::Video = meta.kind {
        msg = msg.attribute(Attribute::Video {
            duration: Duration::from_secs(meta.duration_secs.unwrap_or(0) as u64),
            w: meta.dims.map(|d| d.0).unwrap_or(0),
            h: meta.dims.map(|d| d.1).unwrap_or(0),
            supports_streaming: true,
            round_message: false,
        });
    }

    msg
}

pub fn build_media_input(
    caption: &str,
    meta: &MediaMeta,
    uploaded: Uploaded,
    thumb: Option<Uploaded>,
) -> InputMedia {
    let mut media = InputMedia::new();
    if !caption.is_empty() {
        media = media.html(caption);
    }
    media = media.mime_type(&meta.mime_type);

    if send_as_photo(meta) {
        media = media.photo(uploaded);
    } else {
        media = media.document(uploaded);
        if let Some(t) = thumb {
            media = media.thumbnail(t);
        }
    }

    if let MediaKind::Video = meta.kind {
        media = media.attribute(Attribute::Video {
            duration: Duration::from_secs(meta.duration_secs.unwrap_or(0) as u64),
            w: meta.dims.map(|d| d.0).unwrap_or(0),
            h: meta.dims.map(|d| d.1).unwrap_or(0),
            supports_streaming: true,
            round_message: false,
        });
    }

    media
}
