use std::sync::Arc;
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::bot::style::{build_media_caption, CaptionOptions};
use crate::provider::MediaKind;

pub mod cache;
pub mod uploader;
pub mod url;

pub use cache::*;
pub use uploader::*;
pub use url::*;

pub async fn handle_dl(
    client: &Client,
    chat: PeerRef,
    raw_url: &str,
    format: Option<&str>,
    state: &Arc<AppState>,
    sender_name: Option<String>,
    reply_to_msg_id: Option<i32>,
) -> anyhow::Result<()> {
    let start_time = std::time::Instant::now();
    let _guard = JobGuard::new(state.clone());
    let url = normalize_url(raw_url);
    let cache_key = match format {
        Some(f) => format!("{url}#{f}"),
        None => url.clone(),
    };

    let status_msg = client
        .send_message(
            chat,
            InputMessage::new()
                .text("Resolving URL...")
                .reply_to(reply_to_msg_id),
        )
        .await?;
    let status_id = status_msg.id();

    // 1. Direct cache hit
    if let Some(cached) = state.media_cache.get(&cache_key)
        && !cached.medias.is_empty()
    {
        return send_cached_media(
            client,
            CachedMediaContext {
                chat,
                status_id,
                reply_to_msg_id,
                url: &url,
                cached: &cached,
                sender_name: sender_name.as_deref(),
                start_time,
                is_coalesced: false,
            },
            state,
        )
        .await;
    }

    // 2. In-flight request deduplication: if identical URL is currently downloading, wait for it
    let flight_leader = match state.singleflight.wait_or_lead(&cache_key).await {
        Ok(crate::pool::FlightStatus::Leader(leader)) => Some(leader),
        Ok(crate::pool::FlightStatus::Follower(())) => {
            if let Some(cached) = state.media_cache.get(&cache_key)
                && !cached.medias.is_empty()
            {
                return send_cached_media(
                    client,
                    CachedMediaContext {
                        chat,
                        status_id,
                        reply_to_msg_id,
                        url: &url,
                        cached: &cached,
                        sender_name: sender_name.as_deref(),
                        start_time,
                        is_coalesced: true,
                    },
                    state,
                )
                .await;
            }
            None
        }
        Err(_) => None,
    };

    // Acquire worker slot before heavy downstream network operations
    let client_for_queue = client.clone();
    let _worker_permit = state
        .worker_pool
        .acquire(|queue_pos| {
            let client = client_for_queue;
            tokio::spawn(async move {
                let _ = client
                    .edit_message(
                        chat,
                        status_id,
                        InputMessage::new().html(format!(
                            "⏳ <b>All workers are busy</b>\n\
                             ├ <b>Queue:</b> #{queue_pos}\n\
                             └ Please wait, your download will start shortly...",
                        )),
                    )
                    .await;
            });
        })
        .await?;

    let _ = client
        .edit_message(
            chat,
            status_id,
            InputMessage::new().text("Resolving URL via provider chain..."),
        )
        .await;

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

    #[allow(clippy::needless_range_loop)]
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

    let type_str = if items[0].meta.filename.ends_with(".zip") {
        "ZIP Archive"
    } else {
        match kind {
            MediaKind::Video => "Video",
            MediaKind::Photo => "Photo",
            MediaKind::Audio => "Audio",
        }
    };

    let total_size: u64 = items.iter().map(|item| item.meta.size).sum();
    let resolution = if kind == MediaKind::Video {
        items[0].meta.dims
    } else {
        None
    };

    let caption = build_media_caption(CaptionOptions {
        title: title.as_deref(),
        description: description.as_deref(),
        source_url: &url,
        type_str,
        resolution,
        total_bytes: total_size,
        duration: None,
        elapsed: start_time.elapsed(),
        cache_label: None,
        sender_name: sender_name.as_deref(),
    });

    let (sent_medias, caption_remainder) = if total == 1 {
        let item = items.into_iter().next().unwrap();
        let medias = upload_and_send_single(
            client,
            chat,
            status_id,
            reply_to_msg_id,
            item,
            caption,
            state,
        )
        .await?;
        (medias, None)
    } else {
        upload_and_send_batches(
            client,
            chat,
            status_id,
            reply_to_msg_id,
            items,
            Some(caption),
            state,
        )
        .await?
    };

    if sent_medias.is_empty() {
        state.bot_stats.record_failure();
        let _ = client
            .edit_message(
                chat,
                status_id,
                InputMessage::new().text("Failed to send any resolved media to chat."),
            )
            .await;
        return Err(anyhow::anyhow!("failed to send any media"));
    }

    if let Some(ref cap) = caption_remainder
        && !cap.is_empty()
    {
        let _ = client
            .send_message(
                chat,
                InputMessage::new().html(cap).reply_to(reply_to_msg_id),
            )
            .await;
    }

    state.media_cache.insert(
        cache_key,
        crate::app::CachedMedia {
            medias: sent_medias,
            title,
            description,
            kind,
        },
    );

    if let Some(leader) = flight_leader {
        leader.complete(());
    }

    let _ = client.delete_messages(chat, &[status_id]).await;
    state.bot_stats.record_success();
    Ok(())
}
