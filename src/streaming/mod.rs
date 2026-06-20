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

pub async fn upload_media(
    client: &Client,
    chat: PeerRef,
    status_msg_id: i32,
    meta: MediaMeta,
    reader: MediaReader,
    config: &Config,
    _permit: tokio::sync::OwnedSemaphorePermit,
) -> anyhow::Result<grammers_client::media::Uploaded> {
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
                    let _ = client
                        .edit_message(
                            chat,
                            status_msg_id,
                            InputMessage::new().text(format!(
                                "Uploading: {:.1}% ({} / {} bytes)",
                                pct, read, total
                            )),
                        )
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

    Ok(uploaded)
}

pub fn build_media_message(
    caption: &str,
    meta: &MediaMeta,
    uploaded: Uploaded,
) -> InputMessage {
    let mut msg = InputMessage::new().text(caption);
    msg = msg.mime_type(&meta.mime_type);

    match meta.kind {
        MediaKind::Video | MediaKind::File => {
            msg = msg.document(uploaded);
        }
        MediaKind::Photo => {
            msg = msg.photo(uploaded);
        }
        MediaKind::Audio => {
            msg = msg.document(uploaded);
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
) -> InputMedia {
    let mut media = InputMedia::new();
    if !caption.is_empty() {
        media = media.caption(caption);
    }
    media = media.mime_type(&meta.mime_type);

    match meta.kind {
        MediaKind::Photo => {
            media = media.photo(uploaded);
        }
        MediaKind::Video | MediaKind::Audio | MediaKind::File => {
            media = media.document(uploaded);
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
