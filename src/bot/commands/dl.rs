use std::sync::Arc;

use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::provider::MediaKind;
use crate::streaming;

fn normalize_url(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return url.to_string(),
    };
    let tracking: [&str; 9] = [
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "igsh",
        "igshid",
        "fbclid",
        "gclid",
    ];
    let keep: Vec<_> = parsed
        .query_pairs()
        .filter(|(k, _)| !tracking.contains(&&k[..]))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if keep.is_empty() && parsed.query().is_some() {
        let mut clean = parsed;
        clean.set_query(None);
        return clean.as_str().to_string();
    }
    if keep.is_empty() {
        return parsed.as_str().to_string();
    }
    let mut clean = parsed;
    clean.query_pairs_mut().clear();
    for (k, v) in &keep {
        clean.query_pairs_mut().append_pair(k, v);
    }
    clean.as_str().to_string()
}

struct JobGuard {
    state: Arc<AppState>,
}

impl JobGuard {
    fn new(state: Arc<AppState>) -> Self {
        state.bot_stats.incr_active_jobs();
        Self { state }
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.state.bot_stats.decr_active_jobs();
    }
}

pub async fn handle_dl(
    client: &Client,
    chat: PeerRef,
    url: &str,
    format: Option<&str>,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let _guard = JobGuard::new(state.clone());
    let url = normalize_url(url);

    let status_msg = client
        .send_message(chat, InputMessage::new().text("Resolving URL..."))
        .await?;
    let status_id = status_msg.id();

    tracing::info!(url, ?format, "resolving URL via provider chain");
    let items = match state.registry.resolve_and_fetch(&url, format).await {
        Ok(items) => items,
        Err(e) => {
            state.bot_stats.record_failure();
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text(format!("Error: {e}")),
                )
                .await;
            return Err(e);
        }
    };

    if items.is_empty() {
        let _ = client
            .edit_message(
                chat,
                status_id,
                InputMessage::new().text("Error: no media found"),
            )
            .await;
        return Err(anyhow::anyhow!("no media found"));
    }

    for i in 0..items.len() {
        if items[i].meta.size > state.config.max_file_size_bytes() {
            state.bot_stats.record_failure();
            let err = anyhow::anyhow!(
                "file too large: {} bytes (max {})",
                items[i].meta.size,
                state.config.max_file_size_bytes()
            );
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text(format!("Error: {err}")),
                )
                .await;
            return Err(err);
        }
    }

    let total = items.len();
    let kind = items[0].meta.kind;
    let title = items[0].meta.title.clone();
    let description = items[0].meta.description.clone();

    let emoji = match kind {
        MediaKind::Video => "🎬",
        MediaKind::Audio => "🎵",
        MediaKind::Photo => "🖼️",
        MediaKind::File => "📎",
    };

    let mut caption = String::new();
    if let Some(ref t) = title {
        caption.push_str(&format!("{emoji} {t}\n\n"));
    }
    if let Some(ref desc) = description {
        caption.push_str(desc);
        caption.push_str("\n\n");
    }
    caption.push_str(&format!(
        "🦀 Powered by @{}",
        state.bot_stats.bot_username()
    ));

    let mut uploads: Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)> =
        Vec::with_capacity(total);
    for (i, item) in items.into_iter().enumerate() {
        let _ = client
            .edit_message(
                chat,
                status_id,
                InputMessage::new().text(if total > 1 {
                    format!(
                        "Uploading {}/{}: {} ({} bytes)",
                        i + 1,
                        total,
                        item.meta.filename,
                        item.meta.size
                    )
                } else {
                    format!(
                        "Uploading: {} ({} bytes)",
                        item.meta.filename, item.meta.size
                    )
                }),
            )
            .await;

        let uploaded = match streaming::upload_media(
            client,
            chat,
            status_id,
            item.meta.clone(),
            item.reader,
            &state.config,
        )
        .await
        {
            Ok(u) => u,
            Err(e) => {
                state.bot_stats.record_failure();
                let _ = client
                    .edit_message(
                        chat,
                        status_id,
                        InputMessage::new().text(format!("Upload {}/{} failed: {e}", i + 1, total)),
                    )
                    .await;
                return Err(e);
            }
        };

        uploads.push((uploaded, item.meta));
    }

    if total == 1 {
        let (uploaded, meta) = uploads.into_iter().next().unwrap();
        let media_msg = streaming::build_media_message(&caption, &meta, uploaded);
        let _ = client.send_message(chat, media_msg).await;
    } else {
        let mut photo_uploads: Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)> =
            Vec::new();
        let mut other_uploads: Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)> =
            Vec::new();
        for item in uploads {
            if item.1.kind == MediaKind::Photo {
                photo_uploads.push(item);
            } else {
                other_uploads.push(item);
            }
        }

        const ALBUM_MAX: usize = 10;

        let mut photo_batches: Vec<
            Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)>,
        > = Vec::new();
        while !photo_uploads.is_empty() {
            let batch_size = ALBUM_MAX.min(photo_uploads.len());
            let batch: Vec<_> = photo_uploads.drain(..batch_size).collect();
            photo_batches.push(batch);
        }

        let total_albums = photo_batches.len();
        let total_sends = total_albums + other_uploads.len();

        for (batch_idx, batch) in photo_batches.into_iter().enumerate() {
            let is_last = batch_idx + 1 == total_sends;
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text(format!("Sending album ({} photos)...", batch.len())),
                )
                .await;

            let album_ok = {
                let medias: Vec<_> = batch
                    .iter()
                    .enumerate()
                    .map(|(i, (uploaded, meta))| {
                        let c = if i == 0 && is_last {
                            caption.clone()
                        } else {
                            String::new()
                        };
                        streaming::build_media_input(&c, meta, uploaded.clone())
                    })
                    .collect();
                client.send_album(chat, medias).await.is_ok()
            };

            if !album_ok {
                tracing::warn!("send_album failed, sending individually");
                for (i, (uploaded, meta)) in batch.into_iter().enumerate() {
                    let c = if is_last && i == 0 { &caption } else { "" };
                    let media_msg = streaming::build_media_message(c, &meta, uploaded);
                    let _ = client.send_message(chat, media_msg).await;
                }
            }
        }

        for (i, (uploaded, meta)) in other_uploads.into_iter().enumerate() {
            let is_last = total_albums + i + 1 == total_sends;
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text(format!("Sending: {}", meta.filename)),
                )
                .await;
            let c = if is_last { &caption } else { "" };
            let media_msg = streaming::build_media_message(c, &meta, uploaded);
            let _ = client.send_message(chat, media_msg).await;
        }
    }

    let _ = client.delete_messages(chat, &[status_id]).await;
    state.bot_stats.record_success();
    Ok(())
}
