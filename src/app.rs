use std::sync::Arc;

use grammers_client::Client;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::bot::status::BotStats;
use crate::config::Config;
use crate::provider::astra::AstraProvider;
use crate::provider::registry::ProviderRegistry;

#[derive(Clone)]
pub struct CachedMedia {
    pub medias: Vec<grammers_client::media::Media>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub kind: crate::provider::MediaKind,
}

pub struct AppState {
    pub client: Client,
    pub config: Arc<Config>,
    pub registry: Arc<ProviderRegistry>,
    pub bot_stats: BotStats,
    pub pending_downloads: Arc<dashmap::DashMap<String, String>>,
    pub media_cache: Arc<dashmap::DashMap<String, CachedMedia>>,
    /// Shared HTTP client for lightweight diagnostic calls (/check, /status, /speedtest).
    /// Per-call timeouts are applied per-request via `.timeout(..)` on the builder, so a
    /// single connection pool + TLS state is reused instead of rebuilt on every command.
    pub http: reqwest::Client,
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

        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let state = Arc::new(Self {
            client: session.client,
            config,
            registry,
            bot_stats: BotStats::new(session.bot_username),
            pending_downloads: Arc::new(dashmap::DashMap::new()),
            media_cache: Arc::new(dashmap::DashMap::new()),
            http,
        });

        Ok((state, session.updates_rx))
    }
}
