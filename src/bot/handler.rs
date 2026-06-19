use std::sync::Arc;

use grammers_client::Client;
use grammers_client::message::{InputMessage, Message};

use crate::app::AppState;
use crate::streaming;

pub async fn handle_dl(
    client: &Client,
    msg: &Message,
    url: &str,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let peer = msg
        .peer()
        .ok_or_else(|| anyhow::anyhow!("no peer in message"))?;
    let chat = peer
        .to_ref()
        .await
        .ok()
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("no peer ref"))?;

    let status_msg = client
        .send_message(chat, InputMessage::new().text("Resolving URL..."))
        .await?;

    tracing::info!(url, "resolving URL via provider chain");
    let resolve_result = state.registry.resolve_and_fetch(url).await;

    let (meta, reader) = match resolve_result {
        Ok((meta, reader)) => {
            tracing::info!(
                filename = meta.filename,
                size = meta.size,
                mime = meta.mime_type,
                kind = ?meta.kind,
                "URL resolved"
            );
            (meta, reader)
        }
        Err(e) => {
            state.bot_stats.record_failure();
            let _ = client
                .edit_message(
                    chat,
                    status_msg.id(),
                    InputMessage::new().text(format!("Error: {e}")),
                )
                .await;
            return Err(e);
        }
    };

    if meta.size > state.config.max_file_size_bytes() {
        state.bot_stats.record_failure();
        let err = anyhow::anyhow!(
            "file too large: {} bytes (max {})",
            meta.size,
            state.config.max_file_size_bytes()
        );
        let _ = client
            .edit_message(
                chat,
                status_msg.id(),
                InputMessage::new().text(format!("Error: {err}")),
            )
            .await;
        return Err(err);
    }

    let _ = client
        .edit_message(
            chat,
            status_msg.id(),
            InputMessage::new().text(format!(
                "Downloading: {} ({} bytes)",
                meta.filename, meta.size
            )),
        )
        .await;

    let permit = state
        .job_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow::anyhow!("failed to acquire job permit"))?;

    let result = streaming::upload_media(
        client,
        chat,
        &format!("via @{url}"),
        meta,
        reader,
        &state.config,
        permit,
    )
    .await;

    match result {
        Ok(_sent) => {
            state.bot_stats.record_success();
            let _ = client.delete_messages(chat, &[status_msg.id()]).await;
            Ok(())
        }
        Err(e) => {
            state.bot_stats.record_failure();
            let _ = client
                .edit_message(
                    chat,
                    status_msg.id(),
                    InputMessage::new().text(format!("Upload failed: {e}")),
                )
                .await;
            Err(e)
        }
    }
}
