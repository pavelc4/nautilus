use std::sync::Arc;

use grammers_client::client::UpdatesConfiguration;
use grammers_client::message::InputMessage;
use grammers_client::update::Update;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::AppState;
use crate::bot::handler;
use crate::bot::ratelimit::RateLimiter;
use crate::bot::settings;
use crate::bot::status;

pub async fn run(
    state: Arc<AppState>,
    updates_rx: UnboundedReceiver<grammers_session::updates::UpdatesLike>,
) -> anyhow::Result<()> {
    let rate_limiter = std::sync::Arc::new(RateLimiter::new(
        state.config.rate_limit_tokens(),
        state.config.rate_limit_refill_secs(),
    ));

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
                let rate_limiter = rate_limiter.clone();

                tokio::spawn(async move {
                    tracing::debug!(?update, "received update");
                    if let Err(e) = handle_update(&state, &rate_limiter, update).await {
                        tracing::error!("handler error: {e:#}");
                    }
                });
            }
        }
    }

    Ok(())
}

async fn handle_update(
    state: &Arc<AppState>,
    rate_limiter: &std::sync::Arc<RateLimiter>,
    update: Update,
) -> anyhow::Result<()> {
    match update {
        Update::NewMessage(msg) if msg.outgoing() => Ok(()),
        Update::NewMessage(msg) => handle_message(state, rate_limiter, msg).await,
        _ => Ok(()),
    }
}

async fn handle_message(
    state: &Arc<AppState>,
    rate_limiter: &std::sync::Arc<RateLimiter>,
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

    if !rate_limiter.check(sender_id) {
        let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
        let chat = peer
            .to_ref()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;
        state
            .client
            .send_message(
                chat,
                InputMessage::new().text("Rate limit exceeded. Please wait."),
            )
            .await?;
        return Ok(());
    }

    if !text.starts_with('/') {
        return Ok(());
    }

    let cmd = text.split_whitespace().next().unwrap_or("");
    let whitelist = ["/dl", "/start", "/status", "/settings"];
    if !whitelist.contains(&cmd) {
        return Ok(());
    }

    if let Some(url) = text.strip_prefix("/dl ") {
        let url = url.trim();
        tracing::info!(sender = sender_id, url, "download requested");
        handler::handle_dl(&state.client, &msg, url, state).await?;
    } else if text == "/status" {
        let reply = status::cmd_status(state, &state.client).await?;
        let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
        let chat = peer
            .to_ref()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;
        state.client.send_message(chat, reply).await?;
    } else if text.starts_with("/settings") {
        let args = text.strip_prefix("/settings").unwrap_or("").trim();
        let reply = settings::cmd_settings(state, sender_id, args);
        let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
        let chat = peer
            .to_ref()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;
        state.client.send_message(chat, reply).await?;
    } else if text == "/start" {
        let peer = msg.peer().ok_or_else(|| anyhow::anyhow!("no peer"))?;
        let chat = peer
            .to_ref()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;
        state
            .client
            .send_message(
                chat,
                InputMessage::new().text(
                    "Orion Bot — zero-disk media downloader.\nSend /dl <url> to download media.",
                ),
            )
            .await?;
    }

    Ok(())
}
