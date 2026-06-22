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
    sender_name: Option<String>,
) -> anyhow::Result<()> {
    let _guard = JobGuard::new(state.clone());
    let url = normalize_url(url);
    let cache_key = match format {
        Some(f) => format!("{url}#{f}"),
        None => url.clone(),
    };

    let status_msg = client
        .send_message(chat, InputMessage::new().text("Resolving URL..."))
        .await?;
    let status_id = status_msg.id();

    if let Some(cached) = state.media_cache.get(&cache_key) {
        let cached = cached.value();
        if !cached.medias.is_empty() {
            tracing::info!(url, "cache hit at bot level, sending cached media");
            state.bot_stats.record_cache_hit();
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text("Sending cached media..."),
                )
                .await;

            let mut quote_content = String::new();
            if let Some(ref t) = cached.title {
                let escaped_t = html_escape(t);
                quote_content.push_str(&format!("<b>{escaped_t}</b>\n"));
            }
            if let Some(ref desc) = cached.description {
                let escaped_desc = html_escape(desc);
                quote_content.push_str(&escaped_desc);
            }
            let quote_content = quote_content.trim();

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
            let size_mb = (total_size as f64) / (1024.0 * 1024.0);

            let mut metadata = String::new();
            metadata.push_str(&format!("🔗 Sumber: {}\n", get_source_link(&url)));
            metadata.push_str(&format!("🏷 Tipe: {type_str}\n"));
            metadata.push_str(&format!("💾 Ukuran: {:.2} MB\n", size_mb));
            if let Some(ref name) = sender_name {
                metadata.push_str(&format!("👤 Oleh: {name}\n"));
            }
            metadata.push_str(
                "\n😼 Powered by <a href=\"https://github.com/pavelc4/astra.git\">Astra</a>",
            );

            let caption;
            let mut separate_text = None;

            if !quote_content.is_empty() {
                let blockquote = format!("<blockquote>{quote_content}</blockquote>\n\n");
                if blockquote.chars().count() + metadata.chars().count() > 1000 {
                    caption = metadata;
                    separate_text = Some(blockquote);
                } else {
                    caption = format!("{}{}", blockquote, metadata);
                }
            } else {
                caption = metadata;
            }

            let mut sent_any = false;
            if cached.medias.len() == 1 {
                let media = &cached.medias[0];
                if let Some(im) = media.to_raw_input_media() {
                    let msg = InputMessage::new().html(caption).media(im);
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
                        input_media = input_media.html(&caption);
                    }
                    album_medias.push(input_media);
                }
                if client.send_album(chat, album_medias).await.is_ok() {
                    sent_any = true;
                }
            }

            if sent_any && let Some(ref sep) = separate_text {
                let _ = client
                    .send_message(chat, InputMessage::new().html(sep))
                    .await;
            }

            let _ = client.delete_messages(chat, &[status_id]).await;
            state.bot_stats.record_success();
            return Ok(());
        }
    }

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

    // Index loop, not `&items`: MediaItem holds a `dyn AsyncRead + Send` reader that is
    // not Sync, so `slice::Iter<MediaItem>` is not Send and cannot be held across the
    // .await below inside the spawned (Send) handler task. A `Range<usize>` is Send.
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

    let mut quote_content = String::new();
    if let Some(ref t) = title {
        let escaped_t = html_escape(t);
        quote_content.push_str(&format!("<b>{escaped_t}</b>\n"));
    }
    if let Some(ref desc) = description {
        let escaped_desc = html_escape(desc);
        quote_content.push_str(&escaped_desc);
    }
    let quote_content = quote_content.trim();

    let type_str = match kind {
        MediaKind::Video => "Video",
        MediaKind::Photo => "Photo",
        MediaKind::Audio => "Audio",
    };

    let total_size: u64 = items.iter().map(|item| item.meta.size).sum();
    let size_mb = (total_size as f64) / (1024.0 * 1024.0);

    let mut metadata = String::new();
    metadata.push_str(&format!("🖇️ Source: {}\n", get_source_link(&url)));
    metadata.push_str(&format!("📄 Type: {type_str}\n"));
    metadata.push_str(&format!("📁 Size: {:.2} MB\n", size_mb));
    if let Some(ref name) = sender_name {
        metadata.push_str(&format!("👤 By: {name}\n"));
    }
    metadata.push_str("\n💫 Powered by <a href=\"https://github.com/pavelc4/astra.git\">Astra</a>");

    let caption;
    let mut separate_text = None;

    if !quote_content.is_empty() {
        let blockquote = format!("<blockquote>{quote_content}</blockquote>\n\n");
        if blockquote.chars().count() + metadata.chars().count() > 1000 {
            caption = metadata;
            separate_text = Some(blockquote);
        } else {
            caption = format!("{}{}", blockquote, metadata);
        }
    } else {
        caption = metadata;
    }

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

    let mut sent_medias = Vec::new();
    let mut caption_to_send = Some(caption.clone());

    if total == 1 {
        let (uploaded, meta) = uploads.into_iter().next().unwrap();
        let c = caption_to_send.take().unwrap_or_default();
        let media_msg = streaming::build_media_message(&c, &meta, uploaded);
        match client.send_message(chat, media_msg).await {
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
    } else {
        let mut album_uploads: Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)> =
            Vec::new();
        let mut other_uploads: Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)> =
            Vec::new();
        for item in uploads {
            if item.1.kind == MediaKind::Photo || item.1.kind == MediaKind::Video {
                album_uploads.push(item);
            } else {
                other_uploads.push(item);
            }
        }

        const ALBUM_MAX: usize = 10;

        let mut album_batches: Vec<
            Vec<(grammers_client::media::Uploaded, crate::provider::MediaMeta)>,
        > = Vec::new();
        while !album_uploads.is_empty() {
            let batch_size = if album_uploads.len() == 11 {
                9
            } else if album_uploads.len() == 1 {
                other_uploads.push(album_uploads.remove(0));
                break;
            } else {
                ALBUM_MAX.min(album_uploads.len())
            };
            let batch: Vec<_> = album_uploads.drain(..batch_size).collect();
            album_batches.push(batch);
        }

        let total_albums = album_batches.len();
        let total_sends = total_albums + other_uploads.len();

        for (batch_idx, batch) in album_batches.into_iter().enumerate() {
            let is_last = batch_idx + 1 == total_sends;
            let _ = client
                .edit_message(
                    chat,
                    status_id,
                    InputMessage::new().text(format!("Sending album ({} media)...", batch.len())),
                )
                .await;

            let mut album_sent = false;
            let medias: Vec<_> = batch
                .iter()
                .enumerate()
                .map(|(i, (uploaded, meta))| {
                    let c = if i == 0 && is_last {
                        caption_to_send.clone().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    streaming::build_media_input(&c, meta, uploaded.clone())
                })
                .collect();

            match client.send_album(chat, medias).await {
                Ok(msgs) => {
                    album_sent = true;
                    if is_last {
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
                    tracing::warn!("send_album failed: {e:#}, sending individually");
                }
            }

            if !album_sent {
                for (uploaded, meta) in batch {
                    let c = if is_last && caption_to_send.is_some() {
                        caption_to_send.take().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let media_msg = streaming::build_media_message(&c, &meta, uploaded);
                    match client.send_message(chat, media_msg).await {
                        Ok(msg) => {
                            if let Some(m) = msg.media() {
                                sent_medias.push(m);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("failed to send individual media in fallback: {e:#}");
                            if !c.is_empty() {
                                caption_to_send = Some(c);
                            }
                        }
                    }
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
            let c = if is_last && caption_to_send.is_some() {
                caption_to_send.take().unwrap_or_default()
            } else {
                String::new()
            };
            let media_msg = streaming::build_media_message(&c, &meta, uploaded);
            match client.send_message(chat, media_msg).await {
                Ok(msg) => {
                    if let Some(m) = msg.media() {
                        sent_medias.push(m);
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to send individual other_upload: {e:#}");
                    if !c.is_empty() {
                        caption_to_send = Some(c);
                    }
                }
            }
        }
    }

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

    if let Some(ref cap) = caption_to_send
        && !cap.is_empty()
    {
        let _ = client
            .send_message(chat, InputMessage::new().html(cap))
            .await;
    }

    if let Some(ref sep) = separate_text {
        let _ = client
            .send_message(chat, InputMessage::new().html(sep))
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

    let _ = client.delete_messages(chat, &[status_id]).await;
    state.bot_stats.record_success();
    Ok(())
}

fn detect_platform(url: &str) -> Option<&'static str> {
    let lower = url.to_lowercase();
    match () {
        _ if lower.contains("facebook.com")
            || lower.contains("fb.watch")
            || lower.contains("fb.com") =>
        {
            Some("Facebook")
        }
        _ if lower.contains("instagram.com") || lower.contains("instagr.am") => Some("Instagram"),
        _ if lower.contains("tiktok.com") => Some("TikTok"),
        _ if lower.contains("twitter.com") || lower.contains("x.com") => Some("Twitter"),
        _ if lower.contains("youtube.com") || lower.contains("youtu.be") => Some("YouTube"),
        _ if lower.contains("pinterest.com") || lower.contains("pin.it") => Some("Pinterest"),
        _ if lower.contains("linkedin.com") => Some("LinkedIn"),
        _ if lower.contains("soundcloud.com") => Some("SoundCloud"),
        _ if lower.contains("spotify.com") => Some("Spotify"),
        _ if lower.contains("threads.com") || lower.contains("threads.net") => Some("Threads"),
        _ if lower.contains("terabox.com") || lower.contains("1024terabox.com") => Some("TeraBox"),
        _ => None,
    }
}

fn get_source_link(url: &str) -> String {
    let escaped_url = html_escape(url);
    if let Some(platform) = detect_platform(url) {
        format!("<a href=\"{escaped_url}\">{platform}</a>")
    } else {
        let domain = url::Url::parse(url)
            .ok()
            .and_then(|u| u.domain().map(|d| d.to_string()))
            .unwrap_or_else(|| "Source".to_string());
        let escaped_domain = html_escape(&domain);
        format!("<a href=\"{escaped_url}\">{escaped_domain}</a>")
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
