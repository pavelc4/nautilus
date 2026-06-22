use std::sync::Arc;

use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::config::Config;

pub struct BotSession {
    pub client: Client,
    pub updates_rx: UnboundedReceiver<grammers_session::updates::UpdatesLike>,
    pub bot_username: String,
}

pub async fn build_client(config: &Config) -> anyhow::Result<BotSession> {
    let session_path = config_session_path();
    let session_dir = std::path::Path::new(&session_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid session path"))?;
    tokio::fs::create_dir_all(session_dir).await?;

    let session = Arc::new(SqliteSession::open(&session_path).await?);

    let SenderPool {
        runner,
        updates,
        handle,
    } = SenderPool::new(session, config.telegram_app_id);
    let client = Client::new(handle);
    // Detached: the sender-pool runner must live for the whole process; the runtime
    // outlives it, so we don't need to hold the JoinHandle.
    tokio::spawn(runner.run());

    if !client.is_authorized().await? {
        tracing::info!("signing in as bot...");
        client
            .bot_sign_in(&config.bot_token, &config.telegram_app_hash)
            .await?;
        tracing::info!("signed in successfully");
    }

    let bot_username = match client.get_me().await {
        Ok(user) => {
            let name = user.username().unwrap_or("(unknown)").to_string();
            tracing::info!(
                user_id = user.id().bare_id_unchecked(),
                username = name,
                first_name = user.first_name().unwrap_or("(none)"),
                "bot logged in"
            );
            name
        }
        Err(e) => {
            tracing::warn!("could not fetch bot info: {e}");
            "(unknown)".to_string()
        }
    };

    Ok(BotSession {
        client,
        updates_rx: updates,
        bot_username,
    })
}

fn config_session_path() -> String {
    std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()) + "/session.sqlite"
}
