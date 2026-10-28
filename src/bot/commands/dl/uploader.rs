use std::sync::Arc;
use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::provider::{MediaItem, MediaKind};
use crate::streaming;

pub fn make_chat_action(
    kind: MediaKind,
    is_doc: bool,
) -> grammers_client::tl::enums::SendMessageAction {
    use grammers_client::tl::{enums::SendMessageAction, types};
    if is_doc {
        SendMessageAction::SendMessageUploadDocumentAction(
            types::SendMessageUploadDocumentAction { progress: 0 },
        )
    } else {
        match kind {
            MediaKind::Video => SendMessageAction::SendMessageUploadVideoAction(
                types::SendMessageUploadVideoAction { progress: 0 },
            ),
            MediaKind::Photo => SendMessageAction::SendMessageUploadPhotoAction(
                types::SendMessageUploadPhotoAction { progress: 0 },
            ),
            MediaKind::Audio => SendMessageAction::SendMessageUploadAudioAction(
                types::SendMessageUploadAudioAction { progress: 0 },
            ),
        }
    }
}

pub async fn upload_and_send_single(
    client: &Client,
    chat: PeerRef,
    status_id: i32,
    reply_to_msg_id: Option<i32>,
    item: MediaItem,
    caption: String,
    cache_key: &str,
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
    let is_doc = !streaming::send_as_photo(&meta);

    let upload_fut = std::pin::pin!(streaming::upload_media(
        client,
        chat,
        status_id,
        item.meta,
        item.reader,
        &state.config,
        &state.http,
    ));

    let (upload_res, _) = client
        .action(chat)
        .repeat(|| make_chat_action(meta.kind, is_doc), upload_fut)
        .await;

    let (uploaded, thumb) = match upload_res {
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

    let media_msg = streaming::build_media_message(&caption, &meta, uploaded.clone(), thumb)
        .invert_media(true);
    let mut sent_medias = Vec::new();

    let dump_peer = state.dump_manager.get_peer(client).await.ok().flatten();
    let mut dumped_msg_id = None;
    let mut dumped_media = None;

    if let Some(dump_p) = dump_peer {
        let dump_upload = media_msg.clone();
        match client.send_message(dump_p, dump_upload).await {
            Ok(msg) => {
                if let Some(m) = msg.media() {
                    dumped_msg_id = Some(msg.id());
                    dumped_media = Some(m);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to post to dump channel, falling back to direct send");
            }
        }
    }

    if let (Some(msg_id), Some(media), Some(dump_p)) = (dumped_msg_id, dumped_media, dump_peer) {
        let record = crate::cache::DumpRecord {
            cache_key: cache_key.to_string(),
            hash: crate::cache::compute_cache_hash(cache_key),
            channel_id: dump_p.id.bot_api_dialog_id().unwrap_or(0),
            message_ids: vec![msg_id],
            kind: match meta.kind {
                MediaKind::Video => "video".to_string(),
                MediaKind::Photo => "photo".to_string(),
                MediaKind::Audio => "audio".to_string(),
            },
            title: meta.title.clone(),
            description: meta.description.clone(),
            total_bytes: meta.size,
            dims: meta.dims,
            duration_secs: meta.duration_secs.map(|s| s as u64),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        };
        let _ = state.dump_cdn.insert(&record).await;

        let user_msg = InputMessage::new()
            .html(&caption)
            .invert_media(true)
            .copy_media(&media)
            .reply_to(reply_to_msg_id);

        match client.send_message(chat, user_msg).await {
            Ok(msg) => {
                if let Some(m) = msg.media() {
                    sent_medias.push(m);
                } else {
                    sent_medias.push(media);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "copy_media to user failed, retrying raw send");
                if let Ok(msg) = client
                    .send_message(chat, media_msg.reply_to(reply_to_msg_id))
                    .await
                    && let Some(m) = msg.media()
                {
                    sent_medias.push(m);
                }
            }
        }
    } else {
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
                let doc_msg = InputMessage::new()
                    .html(&caption)
                    .invert_media(true)
                    .document(uploaded);
                match client
                    .send_message(chat, doc_msg.reply_to(reply_to_msg_id))
                    .await
                {
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
    cache_key: &str,
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
    let mut all_dump_msg_ids = Vec::new();
    let dump_peer = state.dump_manager.get_peer(client).await.ok().flatten();

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
            let is_doc = !streaming::send_as_photo(&item.meta);
            let upload_fut = std::pin::pin!(streaming::upload_media(
                client,
                chat,
                status_id,
                item.meta.clone(),
                item.reader,
                &state.config,
                &state.http,
            ));

            let (upload_res, _) = client
                .action(chat)
                .repeat(|| make_chat_action(item.meta.kind, is_doc), upload_fut)
                .await;

            let (uploaded, thumb) = match upload_res {
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
            let media_msg = streaming::build_media_message(&c, &meta, uploaded.clone(), thumb)
                .invert_media(true);

            let mut dumped = None;
            if let Some(dump_p) = dump_peer {
                if let Ok(msg) = client.send_message(dump_p, media_msg.clone()).await {
                    if let Some(m) = msg.media() {
                        dumped = Some((msg.id(), m));
                    }
                }
            }

            if let (Some((msg_id, media)), Some(_)) = (dumped, dump_peer) {
                all_dump_msg_ids.push(msg_id);
                let user_msg = InputMessage::new()
                    .html(&c)
                    .invert_media(true)
                    .copy_media(&media)
                    .reply_to(reply_to_msg_id);
                if let Ok(msg) = client.send_message(chat, user_msg).await {
                    if let Some(m) = msg.media() {
                        sent_medias.push(m);
                    } else {
                        sent_medias.push(media);
                    }
                }
            } else {
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
                        let doc_msg = InputMessage::new()
                            .html(&c)
                            .invert_media(true)
                            .document(uploaded);
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

                let mut dump_album_success = false;
                if let Some(dump_p) = dump_peer {
                    // Send to dump channel first
                    let dump_medias: Vec<_> = album_items
                        .iter()
                        .enumerate()
                        .map(|(i, (uploaded, thumb, meta))| {
                            let c = if i == 0 && is_last_batch && other_items.is_empty() {
                                caption_to_send.clone().unwrap_or_default()
                            } else {
                                String::new()
                            };
                            streaming::build_media_input(&c, meta, uploaded.clone(), thumb.clone())
                        })
                        .collect();

                    if let Ok(msgs) = client.send_album(dump_p, dump_medias).await {
                        dump_album_success = true;
                        let mut user_album = Vec::new();
                        for (idx, msg_opt) in msgs.into_iter().enumerate() {
                            if let Some(msg) = msg_opt {
                                all_dump_msg_ids.push(msg.id());
                                if let Some(m) = msg.media() {
                                    sent_medias.push(m.clone());
                                    let mut input_media =
                                        grammers_client::media::InputMedia::new().copy_media(&m);
                                    if idx == 0 {
                                        if is_last_batch && other_items.is_empty() {
                                            if let Some(ref c) = caption_to_send {
                                                input_media = input_media.html(c);
                                            }
                                        }
                                        input_media = input_media.reply_to(reply_to_msg_id);
                                    }
                                    user_album.push(input_media);
                                }
                            }
                        }

                        if !user_album.is_empty() {
                            if client.send_album(chat, user_album).await.is_ok() {
                                album_sent = true;
                                if is_last_batch && other_items.is_empty() {
                                    caption_to_send = None;
                                }
                            }
                        }
                    }
                }

                if !dump_album_success {
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
            }

            if !album_sent {
                for (uploaded, thumb, meta) in album_items {
                    let c = if is_last_batch && caption_to_send.is_some() && other_items.is_empty() {
                        caption_to_send.take().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let media_msg = streaming::build_media_message(&c, &meta, uploaded.clone(), thumb)
                        .invert_media(true);
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
                            let doc_msg = InputMessage::new()
                                .html(&c)
                                .invert_media(true)
                                .document(uploaded);
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
                let doc_msg = InputMessage::new().html(&c).invert_media(true).document(uploaded);
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

    if let (Some(dump_p), false) = (dump_peer, all_dump_msg_ids.is_empty()) {
        let record = crate::cache::DumpRecord {
            cache_key: cache_key.to_string(),
            hash: crate::cache::compute_cache_hash(cache_key),
            channel_id: dump_p.id.bot_api_dialog_id().unwrap_or(0),
            message_ids: all_dump_msg_ids,
            kind: "photo".to_string(),
            title: None,
            description: None,
            total_bytes: sent_medias.iter().map(|m| m.size().unwrap_or(0) as u64).sum(),
            dims: None,
            duration_secs: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        };
        let _ = state.dump_cdn.insert(&record).await;
    }

    Ok((sent_medias, caption_to_send))
}
