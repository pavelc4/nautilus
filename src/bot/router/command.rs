use std::sync::Arc;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

use crate::app::AppState;
use crate::bot::commands;

pub async fn dispatch_command(
    state: &Arc<AppState>,
    msg: &grammers_client::update::Message,
    chat: PeerRef,
    cmd: &str,
    args: &str,
    sender_id: i64,
) -> anyhow::Result<()> {
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
                    sender_display(msg),
                    Some(msg.id()),
                )
                .await?;
            } else {
                let text = "⚠️ <b>Usage:</b> <code>/dl &lt;url&gt;</code> or <code>/l &lt;url&gt;</code>\n\
                            Example: <code>/dl https://tiktok.com/...</code>";
                state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new().html(text).reply_to(Some(msg.id())),
                    )
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
                    sender_display(msg),
                    Some(msg.id()),
                )
                .await?;
            } else {
                let text = "⚠️ <b>Usage:</b> <code>/mp &lt;url&gt;</code> or <code>/audio &lt;url&gt;</code>\n\
                            Example: <code>/mp https://youtube.com/...</code>";
                state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new().html(text).reply_to(Some(msg.id())),
                    )
                    .await?;
            }
        }
        "/stats" => {
            if sender_id == state.config.owner_id {
                let reply = commands::stats::cmd_stats(state, &state.client).await?;
                state
                    .client
                    .send_message(chat, reply.reply_to(Some(msg.id())))
                    .await?;
            } else {
                state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new()
                            .text("Permission denied.")
                            .reply_to(Some(msg.id())),
                    )
                    .await?;
            }
        }
        "/start" => {
            commands::start::cmd_start(&state.client, chat, Some(msg.id())).await?;
        }
        "/help" => {
            let reply = commands::help::cmd_help();
            state
                .client
                .send_message(chat, reply.reply_to(Some(msg.id())))
                .await?;
        }
        "/about" => {
            let reply = commands::about::cmd_about();
            state
                .client
                .send_message(chat, reply.reply_to(Some(msg.id())))
                .await?;
        }
        "/check" => {
            if sender_id == state.config.owner_id {
                let reply = commands::check::cmd_check(state).await?;
                state
                    .client
                    .send_message(chat, reply.reply_to(Some(msg.id())))
                    .await?;
            } else {
                state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new()
                            .text("Permission denied.")
                            .reply_to(Some(msg.id())),
                    )
                    .await?;
            }
        }
        "/speedtest" => {
            if sender_id == state.config.owner_id {
                let status_msg = state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new()
                            .text("Running speedtest (this may take up to 6 seconds)...")
                            .reply_to(Some(msg.id())),
                    )
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
                    .send_message(
                        chat,
                        InputMessage::new()
                            .text("Permission denied.")
                            .reply_to(Some(msg.id())),
                    )
                    .await?;
            }
        }
        "/settingsid" => {
            if sender_id == state.config.owner_id {
                let is_group = matches!(
                    msg.peer(),
                    Some(grammers_client::peer::Peer::Group(_))
                        | Some(grammers_client::peer::Peer::Channel(_))
                );

                if !is_group {
                    state
                        .client
                        .send_message(
                            chat,
                            InputMessage::new()
                                .text("This command can only be used in groups.")
                                .reply_to(Some(msg.id())),
                        )
                        .await?;
                    return Ok(());
                }

                if args.is_empty() {
                    let map = state.topic_settings.whitelisted_topics.read().await;
                    let current_topics = match map.get(&chat.id.bare_id_unchecked()) {
                        Some(topics) if !topics.is_empty() => {
                            let list = topics
                                .iter()
                                .map(|id| format!("<code>{}</code>", id))
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("Whitelisted Topic IDs: {}", list)
                        }
                        _ => "No restrictions set (Any topic is allowed).".to_string(),
                    };

                    let response = format!(
                        "Nautilus Topic Whitelist Settings\n\n\
                         Current status: {}\n\n\
                         To set whitelist: <code>/settingsid &lt;id1&gt; &lt;id2&gt; ...</code>\n\
                         To clear: <code>/settingsid clear</code>",
                        current_topics
                    );
                    state
                        .client
                        .send_message(
                            chat,
                            InputMessage::new().html(response).reply_to(Some(msg.id())),
                        )
                        .await?;
                } else if args.eq_ignore_ascii_case("clear") {
                    state
                        .topic_settings
                        .set_allowed_topics(chat.id.bare_id_unchecked(), Vec::new())
                        .await?;
                    state
                        .client
                        .send_message(
                            chat,
                            InputMessage::new()
                                .text("Cleared all whitelisted topic IDs for this group. Any topic is now allowed.")
                                .reply_to(Some(msg.id())),
                        )
                        .await?;
                } else {
                    let mut topic_ids = Vec::new();
                    let mut has_error = false;
                    for part in args.split_whitespace() {
                        match part.parse::<i32>() {
                            Ok(id) => topic_ids.push(id),
                            Err(_) => {
                                has_error = true;
                                break;
                            }
                        }
                    }

                    if has_error {
                        state
                            .client
                            .send_message(
                                chat,
                                InputMessage::new()
                                    .text("Error: Topic IDs must be valid integers.")
                                    .reply_to(Some(msg.id())),
                            )
                            .await?;
                    } else {
                        state
                            .topic_settings
                            .set_allowed_topics(chat.id.bare_id_unchecked(), topic_ids.clone())
                            .await?;
                        let list = topic_ids
                            .iter()
                            .map(|id| format!("<code>{}</code>", id))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let msg_text = format!(
                            "Successfully updated whitelisted topic IDs for this group to: {}",
                            list
                        );
                        state
                            .client
                            .send_message(
                                chat,
                                InputMessage::new().html(msg_text).reply_to(Some(msg.id())),
                            )
                            .await?;
                    }
                }
            } else {
                state
                    .client
                    .send_message(
                        chat,
                        InputMessage::new()
                            .text("Permission denied.")
                            .reply_to(Some(msg.id())),
                    )
                    .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn sender_display(msg: &grammers_client::update::Message) -> Option<String> {
    match msg.sender()? {
        grammers_client::peer::Peer::User(user) => {
            Some(crate::bot::style::format_user_link(user))
        }
        _ => None,
    }
}
