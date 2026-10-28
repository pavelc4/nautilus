pub mod callback;
pub mod command;
pub mod message;

pub use callback::*;
pub use command::*;
pub use message::*;

use std::sync::Arc;

use grammers_client::update::Update;
use grammers_mtsender::UpdatesConfiguration;
use tokio::sync::mpsc::Receiver;

use crate::app::AppState;

pub async fn run(
    state: Arc<AppState>,
    updates_rx: Receiver<grammers_session::updates::UpdatesLike>,
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
        Update::NewMessage(msg) => message::handle_message(state, msg).await,
        Update::CallbackQuery(query) => callback::handle_callback_query(state, query).await,
        _ => Ok(()),
    }
}
