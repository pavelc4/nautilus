use std::sync::Arc;

use grammers_client::Client;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::bot::settings::SettingsMap;
use crate::bot::status::BotStats;
use crate::config::Config;
use crate::provider::astra::AstraProvider;
use crate::provider::registry::ProviderRegistry;

pub struct AppState {
    pub client: Client,
    pub config: Arc<Config>,
    pub registry: Arc<ProviderRegistry>,
    pub bot_stats: BotStats,
    pub settings: SettingsMap,
    pub pending_downloads: Arc<dashmap::DashMap<String, String>>,
}

impl AppState {
    pub async fn new(
        config: Config,
    ) -> anyhow::Result<(
        Arc<Self>,
        UnboundedReceiver<grammers_session::updates::UpdatesLike>,
    )> {
        let session = crate::bot::session::build_client(&config).await?;

        let config = Arc::new(config);

        let astra_url = config
            .astra_api_url
            .clone()
            .unwrap_or_else(|| "http://localhost:3000".to_string());
        let astra = AstraProvider::new(astra_url);

        let registry = Arc::new(ProviderRegistry::new(vec![Box::new(astra)]));

        tracing::info!(
            "app initialized: max_file_size={}",
            config.max_file_size_bytes()
        );

        let state = Arc::new(Self {
            client: session.client,
            config,
            registry,
            bot_stats: BotStats::new(session.bot_username, session.bot_id),
            settings: SettingsMap::new(),
            pending_downloads: Arc::new(dashmap::DashMap::new()),
        });

        Ok((state, session.updates_rx))
    }
}
