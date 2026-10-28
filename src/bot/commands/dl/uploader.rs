use std::sync::Arc;
use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::provider::{MediaItem, MediaKind};
use crate::streaming;

pub async fn upload_and_send_single(
    client: &Client,
    chat: PeerRef,
    status_id: i32,
    reply_to_msg_id: Option<i32>,
    item: MediaItem,
    caption: String,
    state: &Arc<AppState>,
) -> anyhow::Result<Vec<Media>> {
    let _ = client
        .edit_message(
            chat,
            status_id,
            InputMessage::new().text(format!(
                "Uploading: {} ({} bytes)",
                item.meta.filename, item.meta.size
            )),
        )
        .await;

    let meta = item.meta.clone();
    let (uploaded, thumb) = match streaming::upload_media(
        client,
        chat,
        status_id,
        item.meta,
        item.reader,
        &state.config,
        &state.http,
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
                    InputMessage::new().text(format!("Upload failed: {e}")),
                )
                .await;
            return Err(e);
        }
    };

    let media_msg = streaming::build_media_message(&caption, &meta, uploaded.clone(), thumb);
    let mut sent_medias = Vec::new();

    match client
        .send_message(chat, media_msg.reply_to(reply_to_msg_id))
        .await
    {
        Ok(msg) => {
            if let Some(m) = msg.media() {
                sent_medias.push(m);
            }
        }
        Err(e) => {
            tracing::warn!("failed to send media as photo/video: {e:#}, retrying as document");
            let doc_msg = InputMessage::new().html(&caption).document(uploaded);
            match client.send_message(chat, doc_msg.reply_to(reply_to_msg_id)).await {
                Ok(msg) => {
                    if let Some(m) = msg.media() {
                        sent_medias.push(m);
                    }
                }
                Err(e) => {
                    state.bot_stats.record_failure();
                    let _ = client
                        .edit_message(
                            chat,
                            status_id,
                            InputMessage::new().text(format!("Failed to send media: {e}")),
                        )
                        .await;
                    return Err(e.into());
                }
            }
        }
    }

    Ok(sent_medias)
}

pub async fn upload_and_send_batches(
    client: &Client,
    chat: PeerRef,
    status_id: i32,
    reply_to_msg_id: Option<i32>,
    items: Vec<MediaItem>,
    mut caption_to_send: Option<String>,
    state: &Arc<AppState>,
) -> anyhow::Result<(Vec<Media>, Option<String>)> {
    const ALBUM_MAX: usize = 10;
    let mut batches: Vec<Vec<MediaItem>> = Vec::new();
    let mut cur_batch = Vec::new();
    for item in items {
        cur_batch.push(item);
        if cur_batch.len() == ALBUM_MAX {
            batches.push(std::mem::take(&mut cur_batch));
        }
    }
    if !cur_batch.is_empty() {
        batches.push(cur_batch);
    }

    let total_batches = batches.len();
    let mut sent_medias = Vec::new();

    for (batch_idx, batch) in batches.into_iter().enumerate() {
        let is_last_batch = batch_idx + 1 == total_batches;
        let batch_len = batch.len();

        let _ = client
            .edit_message(
                chat,
                status_id,
                InputMessage::new().text(format!(
                    "Uploading batch {}/{} ({} items)...",
                    batch_idx + 1,
                    total_batches,
                    batch_len
                )),
            )
            .await;

        let mut batch_uploads = Vec::with_capacity(batch_len);
        for item in batch {
            let (uploaded, thumb) = match streaming::upload_media(
                client,
                chat,
                status_id,
                item.meta.clone(),
                item.reader,
                &state.config,
                &state.http,
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
                            InputMessage::new().text(format!("Upload failed: {e}")),
                        )
                        .await;
                    return Err(e);
                }
            };

            batch_uploads.push((uploaded, thumb, item.meta));
        }

        let _ = client
            .edit_message(
                chat,
                status_id,
                InputMessage::new().text(format!(
                    "Batch {}/{}: Sending to chat...",
                    batch_idx + 1,
                    total_batches
                )),
            )
            .await;

        if batch_uploads.len() == 1 {
            let (uploaded, thumb, meta) = batch_uploads.into_iter().next().unwrap();
            let c = if is_last_batch {
                caption_to_send.take().unwrap_or_default()
            } else {
                String::new()
            };
            let media_msg = streaming::build_media_message(&c, &meta, uploaded.clone(), thumb);
            match client
                .send_message(chat, media_msg.reply_to(reply_to_msg_id))
                .await
            {
                Ok(msg) => {
                    if let Some(m) = msg.media() {
                        sent_medias.push(m);
                    }
                }
                Err(e) => {
                    tracing::warn!("single send failed ({e:#}), retrying as document");
                    let doc_msg = InputMessage::new().html(&c).document(uploaded);
                    if let Ok(msg) = client
                        .send_message(chat, doc_msg.reply_to(reply_to_msg_id))
                        .await
                        && let Some(m) = msg.media()
                    {
                        sent_medias.push(m);
                    }
                }
            }
        } else {
            let mut album_items = Vec::new();
            let mut other_items = Vec::new();
            for item in batch_uploads {
                if (item.2.kind == MediaKind::Photo && streaming::send_as_photo(&item.2))
                    || item.2.kind == MediaKind::Video
                {
                    album_items.push(item);
                } else {
                    other_items.push(item);
                }
            }

            let mut album_sent = false;
            if album_items.len() >= 2 {
                let medias: Vec<_> = album_items
                    .iter()
                    .enumerate()
                    .map(|(i, (uploaded, thumb, meta))| {
                        let c = if i == 0 && is_last_batch && other_items.is_empty() {
                            caption_to_send.clone().unwrap_or_default()
                        } else {
                            String::new()
                        };
                        let mut input = streaming::build_media_input(&c, meta, uploaded.clone(), thumb.clone());
                        if i == 0 {
                            input = input.reply_to(reply_to_msg_id);
                        }
                        input
                    })
                    .collect();

                match client.send_album(chat, medias).await {
                    Ok(msgs) => {
                        album_sent = true;
                        if is_last_batch && other_items.is_empty() {
                            caption_to_send = None;
                        }
                        for msg_opt in msgs {
                            if let Some(msg) = msg_opt
                                && let Some(m) = msg.media()
                            {
                                sent_medias.push(m);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("send_album failed: {e:#}, falling back to individual send");
                    }
                }
            }

            if !album_sent {
                for (uploaded, thumb, meta) in album_items {
                    let c = if is_last_batch && caption_to_send.is_some() && other_items.is_empty() {
                        caption_to_send.take().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let media_msg = streaming::build_media_message(&c, &meta, uploaded.clone(), thumb);
                    match client
                        .send_message(chat, media_msg.reply_to(reply_to_msg_id))
                        .await
                    {
                        Ok(msg) => {
                            if let Some(m) = msg.media() {
                                sent_medias.push(m);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("photo fallback failed ({e:#}), sending as document");
                            let doc_msg = InputMessage::new().html(&c).document(uploaded);
                            if let Ok(msg) = client
                                .send_message(chat, doc_msg.reply_to(reply_to_msg_id))
                                .await
                                && let Some(m) = msg.media()
                            {
                                sent_medias.push(m);
                            }
                        }
                    }
                }
            }

            for (uploaded, _thumb, _meta) in other_items {
                let c = if is_last_batch && caption_to_send.is_some() {
                    caption_to_send.take().unwrap_or_default()
                } else {
                    String::new()
                };
                let doc_msg = InputMessage::new().html(&c).document(uploaded);
                if let Ok(msg) = client
                    .send_message(chat, doc_msg.reply_to(reply_to_msg_id))
                    .await
                    && let Some(m) = msg.media()
                {
                    sent_medias.push(m);
                }
            }
        }
    }

    Ok((sent_medias, caption_to_send))
}
