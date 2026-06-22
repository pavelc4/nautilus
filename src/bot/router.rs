use std::sync::Arc;

use grammers_client::client::UpdatesConfiguration;
use grammers_client::message::InputMessage;
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
            match () {
                _ if data.starts_with("dl:") => {
                    let parts: Vec<&str> = data.split(':').collect();
                    if let [_, format, id] = parts.as_slice() {
                        let url_opt = state.pending_downloads.get(*id).map(|r| r.value().clone());
                        match url_opt {
                            Some(url) => {
                                let _ = query.answer().send().await;

                                if let Ok(Some(chat_ref)) = query.peer_ref().await
                                    && let grammers_client::tl::enums::Update::BotCallbackQuery(
                                        update,
                                    ) = &query.raw
                                {
                                    let _ = state
                                        .client
                                        .delete_messages(chat_ref, &[update.msg_id])
                                        .await;
                                }

                                match *format {
                                    "cancel" => {
                                        state.pending_downloads.remove(*id);
                                    }
                                    _ => {
                                        if let Ok(Some(chat_ref)) = query.peer_ref().await {
                                            let state = state.clone();
                                            let format_clone = format.to_string();
                                            let id_clone = id.to_string();

                                            let sender_name = match query.sender() {
                                                Some(grammers_client::peer::Peer::User(user)) => {
                                                    match user.username() {
                                                        Some(username) => {
                                                            Some(format!("@{}", username))
                                                        }
                                                        None => {
                                                            let first_name =
                                                                user.first_name().unwrap_or("User");
                                                            let escaped_first =
                                                                html_escape(first_name);
                                                            let user_id =
                                                                user.id().bare_id_unchecked();
                                                            Some(format!(
                                                                "<a href=\"tg://user?id={user_id}\">{escaped_first}</a>"
                                                            ))
                                                        }
                                                    }
                                                }
                                                _ => None,
                                            };

                                            tokio::spawn(async move {
                                                state.pending_downloads.remove(&id_clone);
                                                if let Err(e) = commands::dl::handle_dl(
                                                    &state.client,
                                                    chat_ref,
                                                    &url,
                                                    Some(&format_clone),
                                                    &state,
                                                    sender_name,
                                                )
                                                .await
                                                {
                                                    tracing::error!(
                                                        "Failed download callback: {e:#}"
                                                    );
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            None => {
                                let _ = query
                                    .answer()
                                    .alert("Download request expired or invalid.")
                                    .send()
                                    .await;
                            }
                        }
                    }
                }
                _ if data == "cmd:start" || data == "cmd:help" || data == "cmd:about" => {
                    let _ = query.answer().send().await;
                    // Not a let-chain: keeps the `&query.raw` borrow out of the .await below
                    // so the spawned handler future stays Send.
                    if let Ok(Some(chat_ref)) = query.peer_ref().await
                        && let grammers_client::tl::enums::Update::BotCallbackQuery(update) =
                            &query.raw
                    {
                        let reply = match data.as_str() {
                            "cmd:start" => commands::start::cmd_start_msg(),
                            "cmd:help" => commands::help::cmd_help(),
                            "cmd:about" => commands::about::cmd_about(),
                            _ => unreachable!(),
                        };

                        let _ = state
                            .client
                            .edit_message(chat_ref, update.msg_id, reply)
                            .await;
                    }
                }
                _ => {}
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
    // Borrow the text — `url`/`args` below make their own owned copies only when needed,
    // so the whole message body no longer gets cloned on every update.
    let text = msg.text();
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

        if let Some(url) = whitelisted_url {
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

    // Resolve the chat peer reference once for all valid commands
    let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
    let chat = peer
        .to_ref()
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;

    match cmd {
        "/dl" | "/l" => {
            if !args.is_empty() {
                tracing::info!(sender = sender_id, url = args, "download requested");
                commands::dl::handle_dl(
                    &state.client,
                    chat,
                    args,
                    None,
                    state,
                    sender_display(&msg),
                )
                .await?;
            } else {
                let text = "⚠️ <b>Usage:</b> <code>/dl &lt;url&gt;</code> or <code>/l &lt;url&gt;</code>\n\
                            Example: <code>/dl https://tiktok.com/...</code>";
                state
                    .client
                    .send_message(chat, InputMessage::new().html(text))
                    .await?;
            }
        }
        "/mp" | "/audio" => {
            if !args.is_empty() {
                tracing::info!(sender = sender_id, url = args, "audio download requested");
                commands::dl::handle_dl(
                    &state.client,
                    chat,
                    args,
                    Some("audio"),
                    state,
                    sender_display(&msg),
                )
                .await?;
            } else {
                let text = "⚠️ <b>Usage:</b> <code>/mp &lt;url&gt;</code> or <code>/audio &lt;url&gt;</code>\n\
                            Example: <code>/mp https://youtube.com/...</code>";
                state
                    .client
                    .send_message(chat, InputMessage::new().html(text))
                    .await?;
            }
        }
        "/stats" => {
            if sender_id == state.config.owner_id {
                let reply = commands::stats::cmd_stats(state, &state.client).await?;
                state.client.send_message(chat, reply).await?;
            } else {
                state
                    .client
                    .send_message(chat, "Permission denied.")
                    .await?;
            }
        }
        "/start" => {
            commands::start::cmd_start(&state.client, chat).await?;
        }
        "/help" => {
            let reply = commands::help::cmd_help();
            state.client.send_message(chat, reply).await?;
        }
        "/about" => {
            let reply = commands::about::cmd_about();
            state.client.send_message(chat, reply).await?;
        }
        "/check" => {
            if sender_id == state.config.owner_id {
                let reply = commands::check::cmd_check(state).await?;
                state.client.send_message(chat, reply).await?;
            } else {
                state
                    .client
                    .send_message(chat, "Permission denied.")
                    .await?;
            }
        }
        "/speedtest" => {
            if sender_id == state.config.owner_id {
                let status_msg = state
                    .client
                    .send_message(chat, "Running speedtest (this may take up to 6 seconds)...")
                    .await?;
                match commands::speedtest::cmd_speedtest(state).await {
                    Ok(reply) => {
                        let _ = state
                            .client
                            .edit_message(chat, status_msg.id(), reply)
                            .await;
                    }
                    Err(e) => {
                        let _ = state
                            .client
                            .edit_message(chat, status_msg.id(), format!("Speedtest failed: {e}"))
                            .await;
                    }
                }
            } else {
                state
                    .client
                    .send_message(chat, "Permission denied.")
                    .await?;
            }
        }
        _ => {}
    }

    Ok(())
}

/// Build the sender mention only when a handler actually needs it (the download
/// branches), instead of allocating a `format!` string for every incoming message.
fn sender_display(msg: &grammers_client::update::Message) -> Option<String> {
    match msg.sender()? {
        grammers_client::peer::Peer::User(user) => {
            if let Some(username) = user.username() {
                Some(format!("@{}", username))
            } else {
                let first_name = user.first_name().unwrap_or("User");
                let escaped_first = html_escape(first_name);
                let user_id = user.id().bare_id_unchecked();
                Some(format!(
                    "<a href=\"tg://user?id={user_id}\">{escaped_first}</a>"
                ))
            }
        }
        _ => None,
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
