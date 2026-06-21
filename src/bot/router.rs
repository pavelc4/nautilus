use std::sync::Arc;

use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::AppState;
use crate::bot::commands;

pub async fn run(
    state: Arc<AppState>,
    updates_rx: UnboundedReceiver<grammers_session::updates::UpdatesLike>,
) -> anyhow::Result<()> {
    tracing::info!("starting update loop");

    let mut updates: grammers_client::client::UpdateStream = state
        .client
        .stream_updates(updates_rx, UpdatesConfiguration::default())
        .await
        .map_err(|e| anyhow::anyhow!("stream_updates failed: {e}"))?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal");
                break;
            }
            update = updates.next() => {
                let update = match update {
                    Ok(u) => u,
                    Err(e) => {
                        tracing::error!("update error: {e}");
                        continue;
                    }
                };

                let state = state.clone();

                tokio::spawn(async move {
                    tracing::debug!(?update, "received update");
                    if let Err(e) = handle_update(&state, update).await {
                        tracing::error!("handler error: {e:#}");
                    }
                });
            }
        }
    }

    Ok(())
}

async fn handle_update(state: &Arc<AppState>, update: Update) -> anyhow::Result<()> {
    match update {
        Update::NewMessage(msg) if msg.outgoing() => Ok(()),
        Update::NewMessage(msg) => handle_message(state, msg).await,
        Update::CallbackQuery(query) => {
            let data = String::from_utf8_lossy(query.data()).to_string();
            if data.starts_with("dl:") {
                let parts: Vec<&str> = data.split(':').collect();
                if parts.len() == 3 {
                    let format = parts[1].to_string();
                    let id = parts[2].to_string();

                    let url_opt = state.pending_downloads.get(&id).map(|r| r.value().clone());
                    if let Some(url) = url_opt {
                        let _ = query.answer().send().await;

                        if let Ok(Some(chat_ref)) = query.peer_ref().await {
                            if let grammers_client::tl::enums::Update::BotCallbackQuery(update) =
                                &query.raw
                            {
                                let _ = state
                                    .client
                                    .delete_messages(chat_ref, &[update.msg_id])
                                    .await;
                            }
                        }

                        if format == "cancel" {
                            state.pending_downloads.remove(&id);
                        } else {
                            if let Ok(Some(chat_ref)) = query.peer_ref().await {
                                let state = state.clone();
                                let format_clone = format.clone();
                                let id_clone = id.clone();
                                tokio::spawn(async move {
                                    state.pending_downloads.remove(&id_clone);
                                    if let Err(e) = commands::dl::handle_dl(
                                        &state.client,
                                        chat_ref,
                                        &url,
                                        Some(&format_clone),
                                        &state,
                                    )
                                    .await
                                    {
                                        tracing::error!("Failed download callback: {e:#}");
                                    }
                                });
                            }
                        }
                    } else {
                        let _ = query
                            .answer()
                            .alert("Download request expired or invalid.")
                            .send()
                            .await;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn handle_message(
    state: &Arc<AppState>,
    msg: grammers_client::update::Message,
) -> anyhow::Result<()> {
    let text = msg.text().to_string();
    let sender_id = msg
        .sender()
        .and_then(|p| match p {
            grammers_client::peer::Peer::User(u) => Some(u.id().bare_id_unchecked()),
            _ => None,
        })
        .unwrap_or(0i64);

    if !text.starts_with('/') {
        let whitelisted_url = text.split_whitespace().find(|word| {
            let has_proto = word.starts_with("http://") || word.starts_with("https://");
            has_proto
                && url::Url::parse(word)
                    .map(|parsed| state.registry.resolve(parsed.as_str()).is_ok())
                    .unwrap_or(false)
        });

        match whitelisted_url {
            Some(url) => {
                let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
                let chat = match peer.to_ref().await.ok().flatten() {
                    Some(c) => c,
                    None => anyhow::bail!("no peer ref"),
                };

                tracing::info!(
                    sender = sender_id,
                    url,
                    "auto-detected link: checking format"
                );
                let status_msg = state
                    .client
                    .send_message(chat, "Checking link media...")
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

                        let mut buttons = Vec::new();
                        let mut media_row = Vec::new();
                        if info.has_video {
                            media_row.push(grammers_client::message::Button::data(
                                "Video",
                                format!("dl:video:{}", id),
                            ));
                        }
                        if info.has_audio {
                            media_row.push(grammers_client::message::Button::data(
                                "Audio",
                                format!("dl:audio:{}", id),
                            ));
                        }
                        if !media_row.is_empty() {
                            buttons.push(media_row);
                        }

                        if info.has_photo {
                            buttons.push(vec![grammers_client::message::Button::data(
                                "Photo",
                                format!("dl:photo:{}", id),
                            )]);
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
                            .edit_message(
                                chat,
                                status_msg.id(),
                                format!("Failed to check link: {e}"),
                            )
                            .await?;
                    }
                }
            }
            None => {}
        }
        return Ok(());
    }

    let cmd = text.split_whitespace().next().unwrap_or("");
    let whitelist = ["/dl", "/start", "/status", "/settings"];
    if !whitelist.contains(&cmd) {
        return Ok(());
    }

    // Resolve the chat peer reference once for all valid commands
    let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
    let chat = peer
        .to_ref()
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;

    match cmd {
        "/dl" => {
            let url = text.strip_prefix("/dl ").unwrap_or("").trim();
            if !url.is_empty() {
                tracing::info!(sender = sender_id, url, "download requested");
                commands::dl::handle_dl(&state.client, chat, url, None, state).await?;
            }
        }
        "/status" => {
            let reply = commands::status::cmd_status(state, &state.client).await?;
            state.client.send_message(chat, reply).await?;
        }
        "/settings" => {
            let args = text.strip_prefix("/settings").unwrap_or("").trim();
            let reply = commands::settings::cmd_settings(state, sender_id, args);
            state.client.send_message(chat, reply).await?;
        }
        "/start" => {
            commands::start::cmd_start(&state.client, chat).await?;
        }
        _ => {}
    }

    Ok(())
}
