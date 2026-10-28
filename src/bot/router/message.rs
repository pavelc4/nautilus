use std::sync::Arc;
use grammers_client::message::InputMessage;

use crate::app::AppState;
use super::command::dispatch_command;

pub async fn handle_message(
    state: &Arc<AppState>,
    msg: grammers_client::update::Message,
) -> anyhow::Result<()> {
    let text = msg.text();
    let sender_id = msg
        .sender()
        .and_then(|p| match p {
            grammers_client::peer::Peer::User(u) => Some(u.id().bare_id_unchecked()),
            _ => None,
        })
        .unwrap_or(0i64);

    let hash_query = if text.starts_with("/hash") {
        text.split_whitespace().nth(1)
    } else if !text.starts_with('/') {
        let trimmed = text.trim();
        if (trimmed.len() == 16 || trimmed.len() == 64)
            && trimmed.chars().all(|c| c.is_ascii_hexdigit())
        {
            Some(trimmed)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(hash) = hash_query {
        let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
        let chat = match peer.to_ref().await.ok().flatten() {
            Some(c) => c,
            None => anyhow::bail!("no peer ref"),
        };

        let status = state
            .client
            .send_message(
                chat,
                InputMessage::new()
                    .text("Searching CDN storage by hash...")
                    .reply_to(Some(msg.id())),
            )
            .await?;

        let sender_display = super::command::sender_display(&msg);
        if let Ok(Some(record)) = state.dump_cdn.get_by_hash(hash).await
            && let Ok(Some(dump_peer)) = state.dump_manager.get_peer(&state.client).await
            && crate::bot::commands::dl::send_dump_cdn_cached_media(
                &state.client,
                crate::bot::commands::dl::DumpCdnContext {
                    chat,
                    dump_peer,
                    status_id: status.id(),
                    reply_to_msg_id: Some(msg.id()),
                    url: &record.cache_key,
                    record: &record,
                    sender_name: sender_display.as_deref(),
                    start_time: std::time::Instant::now(),
                },
                state,
            )
            .await?
        {
            return Ok(());
        }

        let _ = state
            .client
            .edit_message(
                chat,
                status.id(),
                InputMessage::new().html(format!(
                    "❌ <b>Media not found in CDN cache for hash:</b> <code>{hash}</code>"
                )),
            )
            .await;
        return Ok(());
    }

    if !text.starts_with('/') {
        let whitelisted_url = text.split_whitespace().find(|word| {
            let has_proto = word.starts_with("http://") || word.starts_with("https://");
            has_proto
                && url::Url::parse(word)
                    .map(|parsed| state.registry.resolve(parsed.as_str()).is_ok())
                    .unwrap_or(false)
        });

        if let Some(url) = whitelisted_url {
            let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
            let chat = match peer.to_ref().await.ok().flatten() {
                Some(c) => c,
                None => anyhow::bail!("no peer ref"),
            };

            // Check if topic is allowed
            let topic_id = crate::bot::topic_settings::get_message_topic_id(&msg);
            if !state
                .topic_settings
                .is_topic_allowed(chat.id.bare_id_unchecked(), topic_id)
                .await
            {
                return Ok(());
            }

            tracing::info!(
                sender = sender_id,
                url,
                "auto-detected link: checking format"
            );
            let status_msg = state
                .client
                .send_message(
                    chat,
                    InputMessage::new()
                        .text("Checking link media...")
                        .reply_to(Some(msg.id())),
                )
                .await?;

            match state.registry.fetch_metadata(url).await {
                Ok(info) => {
                    if !info.has_video && !info.has_audio && !info.has_photo {
                        state
                            .client
                            .edit_message(
                                chat,
                                status_msg.id(),
                                "No downloadable media found for this link.",
                            )
                            .await?;
                        return Ok(());
                    }

                    let id: String = {
                        use rand::Rng;
                        rand::thread_rng()
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(8)
                            .map(|b| b as char)
                            .collect()
                    };

                    state.pending_downloads.insert(id.clone(), url.to_string());

                    let is_pixiv = url.contains("pixiv.net") || url.contains("pximg.net");

                    let mut buttons = Vec::new();
                    let mut media_row = Vec::new();

                    let has_both = info.has_video && info.has_photo;

                    if info.has_video {
                        media_row.push(grammers_client::message::Button::data(
                            "Video",
                            format!("dl:video:{}", id),
                        ));
                    }
                    if info.has_photo {
                        media_row.push(grammers_client::message::Button::data(
                            "Photo",
                            format!("dl:photo:{}", id),
                        ));
                        if is_pixiv && info.photo_count > 1 {
                            media_row.push(grammers_client::message::Button::data(
                                "ZIP",
                                format!("dl:zip:{}", id),
                            ));
                        }
                    }
                    if !media_row.is_empty() {
                        buttons.push(media_row);
                    }

                    let mut extra_row = Vec::new();
                    if has_both {
                        extra_row.push(grammers_client::message::Button::data(
                            "Download All",
                            format!("dl:both:{}", id),
                        ));
                    }
                    if info.has_audio {
                        extra_row.push(grammers_client::message::Button::data(
                            "Audio",
                            format!("dl:audio:{}", id),
                        ));
                    }
                    if !extra_row.is_empty() {
                        buttons.push(extra_row);
                    }

                    buttons.push(vec![grammers_client::message::Button::data(
                        "Cancel",
                        format!("dl:cancel:{}", id),
                    )]);

                    let markup = grammers_client::message::ReplyMarkup::from_buttons(&buttons);

                    state
                        .client
                        .edit_message(
                            chat,
                            status_msg.id(),
                            grammers_client::message::InputMessage::new()
                                .text("Link detected! Please select what you want to download:")
                                .reply_markup(markup),
                        )
                        .await?;
                }
                Err(e) => {
                    state
                        .client
                        .edit_message(chat, status_msg.id(), format!("Failed to check link: {e}"))
                        .await?;
                }
            }
        }
        return Ok(());
    }

    let first_word = text.split_whitespace().next().unwrap_or("");
    let mut cmd = first_word;
    let mut is_for_me = true;

    if first_word.starts_with('/')
        && let Some(idx) = first_word.find('@')
    {
        let (base_cmd, bot_part) = first_word.split_at(idx);
        let bot_name = &bot_part[1..]; // skip '@'
        cmd = base_cmd;
        is_for_me = bot_name.eq_ignore_ascii_case(state.bot_stats.bot_username());
    }

    let whitelist = [
        "/dl",
        "/l",
        "/mp",
        "/audio",
        "/start",
        "/stats",
        "/check",
        "/speedtest",
        "/settingsid",
        "/help",
        "/about",
    ];
    if !is_for_me || !whitelist.contains(&cmd) {
        return Ok(());
    }

    let args = text
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim())
        .unwrap_or("");

    let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
    let chat = peer
        .to_ref()
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;

    // Check if topic is allowed (exempt /settingsid)
    if cmd != "/settingsid" {
        let topic_id = crate::bot::topic_settings::get_message_topic_id(&msg);
        if !state
            .topic_settings
            .is_topic_allowed(chat.id.bare_id_unchecked(), topic_id)
            .await
        {
            return Ok(());
        }
    }

    dispatch_command(state, &msg, chat, cmd, args, sender_id).await
}
