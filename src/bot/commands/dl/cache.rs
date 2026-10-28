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
    let cap_res = build_media_caption(CaptionOptions {
        title: cached.title.as_deref(),
        description: cached.description.as_deref(),
        source_url: url,
        type_str,
        resolution,
        total_bytes: total_size,
        duration: None,
        elapsed: start_time.elapsed(),
        cache_label: Some(cache_label),
        media_hash: None,
        sender_name,
        bot_username: Some(state.bot_stats.bot_username()),
    });
    let caption = cap_res.caption;
    let separate_text = cap_res.separate_text;

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
        if let Some(ref sep) = separate_text {
            let _ = client
                .send_message(
                    chat,
                    InputMessage::new().html(sep).reply_to(reply_to_msg_id),
                )
                .await;
        }
        let _ = client.delete_messages(chat, &[status_id]).await;
        state.bot_stats.record_success();
    }
    Ok(())
}

pub struct DumpCdnContext<'a> {
    pub chat: PeerRef,
    pub dump_peer: PeerRef,
    pub status_id: i32,
    pub reply_to_msg_id: Option<i32>,
    pub url: &'a str,
    pub record: &'a crate::cache::DumpRecord,
    pub sender_name: Option<&'a str>,
    pub start_time: std::time::Instant,
}

pub async fn send_dump_cdn_cached_media(
    client: &Client,
    ctx: DumpCdnContext<'_>,
    state: &Arc<AppState>,
) -> anyhow::Result<bool> {
    let DumpCdnContext {
        chat,
        dump_peer,
        status_id,
        reply_to_msg_id,
        url,
        record,
        sender_name,
        start_time,
    } = ctx;

    tracing::info!(
        url,
        message_ids = ?record.message_ids,
        "dump CDN cache hit! Fetching from storage channel"
    );
    state.bot_stats.record_cache_hit();

    let _ = client
        .edit_message(
            chat,
            status_id,
            InputMessage::new().text("Fetching from CDN storage..."),
        )
        .await;

    let fetched = match client.get_messages_by_id(dump_peer, &record.message_ids).await {
        Ok(msgs) => msgs,
        Err(e) => {
            tracing::warn!(error = %e, "failed to query dump channel messages");
            return Ok(false);
        }
    };

    let mut valid_medias = Vec::new();
    for msg_opt in fetched {
        match msg_opt.and_then(|m| m.media()) {
            Some(media) => valid_medias.push(media),
            None => {
                tracing::warn!(
                    url,
                    "cached message in dump channel was deleted or has no media, purging record"
                );
                let _ = state.dump_cdn.delete(&record.cache_key).await;
                return Ok(false);
            }
        }
    }

    if valid_medias.is_empty() {
        return Ok(false);
    }

    let type_str = match record.kind.as_str() {
        "video" => "Video",
        "photo" => "Photo",
        "audio" => "Audio",
        "zip" => "ZIP Archive",
        _ => "Media",
    };

    let cap_res = build_media_caption(CaptionOptions {
        title: record.title.as_deref(),
        description: record.description.as_deref(),
        source_url: url,
        type_str,
        resolution: record.dims,
        total_bytes: record.total_bytes,
        duration: record.duration_secs.map(std::time::Duration::from_secs),
        elapsed: start_time.elapsed(),
        cache_label: Some("cdn cache"),
        media_hash: None,
        sender_name,
        bot_username: Some(state.bot_stats.bot_username()),
    });
    let caption = cap_res.caption;
    let separate_text = cap_res.separate_text;

    let mut sent_any = false;
    if valid_medias.len() == 1 {
        let media = &valid_medias[0];
        let msg = InputMessage::new()
            .html(caption)
            .copy_media(media)
            .reply_to(reply_to_msg_id);
        if client.send_message(chat, msg).await.is_ok() {
            sent_any = true;
        }
    } else {
        let mut album_medias = Vec::new();
        for (idx, media) in valid_medias.iter().enumerate() {
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
        if let Some(ref sep) = separate_text {
            let _ = client
                .send_message(
                    chat,
                    InputMessage::new().html(sep).reply_to(reply_to_msg_id),
                )
                .await;
        }
        let _ = client.delete_messages(chat, &[status_id]).await;
        state.bot_stats.record_success();

        let kind = match record.kind.as_str() {
            "photo" => MediaKind::Photo,
            "audio" => MediaKind::Audio,
            _ => MediaKind::Video,
        };
        state.media_cache.insert(
            record.cache_key.clone(),
            crate::app::CachedMedia {
                medias: valid_medias,
                title: record.title.clone(),
                description: record.description.clone(),
                kind,
            },
        );

        return Ok(true);
    }

    Ok(false)
}
