use std::sync::Arc;
use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::bot::style::{build_media_caption, CaptionOptions};
use crate::provider::MediaKind;

pub struct JobGuard {
    state: Arc<AppState>,
}

impl JobGuard {
    pub fn new(state: Arc<AppState>) -> Self {
        state.bot_stats.incr_active_jobs();
        state.bot_stats.incr_active_workers();
        Self { state }
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.state.bot_stats.decr_active_jobs();
        self.state.bot_stats.decr_active_workers();
    }
}

pub struct CachedMediaContext<'a> {
    pub chat: PeerRef,
    pub status_id: i32,
    pub reply_to_msg_id: Option<i32>,
    pub url: &'a str,
    pub cached: &'a crate::app::CachedMedia,
    pub sender_name: Option<&'a str>,
    pub start_time: std::time::Instant,
    pub is_coalesced: bool,
}

pub async fn send_cached_media(
    client: &Client,
    ctx: CachedMediaContext<'_>,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let CachedMediaContext {
        chat,
        status_id,
        reply_to_msg_id,
        url,
        cached,
        sender_name,
        start_time,
        is_coalesced,
    } = ctx;
    tracing::info!(url, is_coalesced, "cache hit at bot level, sending cached media");
    state.bot_stats.record_cache_hit();
    let _ = client
        .edit_message(
            chat,
            status_id,
            InputMessage::new().text("Sending cached media..."),
        )
        .await;

    let type_str = match cached.kind {
        MediaKind::Video => "Video",
        MediaKind::Photo => "Photo",
        MediaKind::Audio => "Audio",
    };

    let total_size: u64 = cached
        .medias
        .iter()
        .map(|m| m.size().unwrap_or(0) as u64)
        .sum();

    let mut resolution = None;
    if cached.kind == MediaKind::Video {
        for m in &cached.medias {
            if let grammers_client::media::Media::Document(doc) = m
                && let Some(res) = doc.resolution()
            {
                resolution = Some(res);
                break;
            }
        }
    }

    let cache_label = if is_coalesced { "coalesced" } else { "cached" };
    let caption = build_media_caption(CaptionOptions {
        title: cached.title.as_deref(),
        description: cached.description.as_deref(),
        source_url: url,
        type_str,
        resolution,
        total_bytes: total_size,
        duration: None,
        elapsed: start_time.elapsed(),
        cache_label: Some(cache_label),
        sender_name,
    });

    let mut sent_any = false;
    if cached.medias.len() == 1 {
        let media = &cached.medias[0];
        if let Some(im) = media.to_raw_input_media() {
            let msg = InputMessage::new()
                .html(caption)
                .media(im)
                .reply_to(reply_to_msg_id);
            if client.send_message(chat, msg).await.is_ok() {
                sent_any = true;
            }
        }
    } else {
        let mut album_medias = Vec::new();
        for (idx, media) in cached.medias.iter().enumerate() {
            let mut input_media =
                grammers_client::media::InputMedia::new().copy_media(media);
            if idx == 0 {
                input_media = input_media.html(&caption).reply_to(reply_to_msg_id);
            }
            album_medias.push(input_media);
        }
        if client.send_album(chat, album_medias).await.is_ok() {
            sent_any = true;
        }
    }

    if sent_any {
        let _ = client.delete_messages(chat, &[status_id]).await;
        state.bot_stats.record_success();
    }
    Ok(())
}
