use std::sync::atomic::Ordering;
use std::time::Duration;

use grammers_client::Client;
use grammers_client::media::Attribute;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::config::Config;
use crate::provider::{MediaKind, MediaMeta, MediaReader};
use crate::streaming::progress::ProgressReader;

pub mod progress;

pub async fn upload_media(
    client: &Client,
    chat: PeerRef,
    caption: &str,
    meta: MediaMeta,
    reader: MediaReader,
    config: &Config,
    _permit: tokio::sync::OwnedSemaphorePermit,
) -> anyhow::Result<grammers_client::message::Message> {
    let (mut progress_reader, byte_counter) = ProgressReader::new(reader);

    let size = meta.size as usize;
    let filename = meta.filename.clone();

    let progress_task = if meta.size > 0 {
        let edit_interval = Duration::from_secs(config.progress_edit_secs());
        let total = meta.size;
        let byte_counter = byte_counter.clone();
        let client = client.clone();

        let status_msg = client
            .send_message(
                chat,
                InputMessage::new().text(format!(
                    "Uploading: {} / {}",
                    byte_counter.load(Ordering::Relaxed),
                    total
                )),
            )
            .await
            .ok();

        status_msg.map(|status_msg| {
            tokio::spawn(async move {
                let mut last_edited = tokio::time::Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let read = byte_counter.load(Ordering::Relaxed);
                    if read >= total {
                        let _ = client
                            .edit_message(
                                chat,
                                status_msg.id(),
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
                                status_msg.id(),
                                InputMessage::new().text(format!(
                                    "Uploading: {:.1}% ({} / {} bytes)",
                                    pct, read, total
                                )),
                            )
                            .await;
                        last_edited = tokio::time::Instant::now();
                    }
                }
            })
        })
    } else {
        None
    };

    let uploaded = client
        .upload_stream(&mut progress_reader, size, filename)
        .await?;

    if let Some(handle) = progress_task {
        handle.abort();
    }

    let mut msg = InputMessage::new().text(caption);

    msg = msg.mime_type(&meta.mime_type);

    match meta.kind {
        MediaKind::Video | MediaKind::File => {
            msg = msg.document(uploaded.clone());
        }
        MediaKind::Photo => {
            msg = msg.photo(uploaded.clone());
        }
        MediaKind::Audio => {
            msg = msg.document(uploaded.clone());
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

    let sent = client.send_message(chat, msg).await?;
    Ok(sent)
}
