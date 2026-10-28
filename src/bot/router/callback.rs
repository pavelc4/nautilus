use std::sync::Arc;
use grammers_client::update::CallbackQuery;

use crate::app::AppState;
use crate::bot::commands;
use crate::bot::style::html_escape;

pub async fn handle_callback_query(
    state: &Arc<AppState>,
    query: CallbackQuery,
) -> anyhow::Result<()> {
    let data = String::from_utf8_lossy(query.data()).to_string();
    match () {
        _ if data.starts_with("dl:") => {
            let parts: Vec<&str> = data.split(':').collect();
            // dl:{format}:{id}
            let (format, id): (String, String) = match parts.as_slice() {
                [_, format, id] => (format.to_string(), id.to_string()),
                _ => {
                    let _ = query.answer().alert("Invalid download request.").send().await;
                    return Ok(());
                }
            };
            let url_opt = state.pending_downloads.get(&id).map(|r| r.value().clone());
            match url_opt {
                Some(url) => {
                    let _ = query.answer().send().await;

                    let reply_to_id = if let Ok(buttons_msg) = query.load_message().await {
                        buttons_msg.reply_to_message_id().or_else(|| {
                            crate::bot::topic_settings::get_topic_id_from_raw(&buttons_msg.raw)
                        })
                    } else {
                        None
                    };

                    if let Ok(Some(chat_ref)) = query.peer_ref().await
                        && let grammers_client::tl::enums::Update::BotCallbackQuery(update) =
                            &query.raw
                    {
                        let _ = state.client.delete_messages(chat_ref, &[update.msg_id]).await;
                    }

                    match format.as_str() {
                        "cancel" => {
                            state.pending_downloads.remove(&id);
                        }
                        _ => {
                            if let Ok(Some(chat_ref)) = query.peer_ref().await {
                                let state = state.clone();
                                let format_clone = format.clone();
                                let id_clone = id.clone();

                                let sender_name = match query.sender() {
                                    Some(grammers_client::peer::Peer::User(user)) => {
                                        match user.username() {
                                            Some(username) => Some(format!("@{}", username)),
                                            None => {
                                                let first_name =
                                                    user.first_name().unwrap_or("User");
                                                let escaped_first = html_escape(first_name);
                                                let user_id = user.id().bare_id_unchecked();
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
                                        reply_to_id,
                                    )
                                    .await
                                    {
                                        tracing::error!("Failed download callback: {e:#}");
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
        _ if data == "cmd:start" || data == "cmd:help" || data == "cmd:about" => {
            let _ = query.answer().send().await;
            if let Ok(Some(chat_ref)) = query.peer_ref().await
                && let grammers_client::tl::enums::Update::BotCallbackQuery(update) = &query.raw
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
