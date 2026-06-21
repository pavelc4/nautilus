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
                commands::dl::handle_dl(&state.client, chat, url, state).await?;
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
