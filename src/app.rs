use std::sync::Arc;

use grammers_client::Client;
use tokio::sync::mpsc::Receiver;

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
    pub worker_pool: Arc<crate::pool::WorkerPool>,
    pub singleflight: Arc<crate::pool::SingleFlight<()>>,
    pub pending_downloads: Arc<dashmap::DashMap<String, String>>,
    pub media_cache: Arc<crate::cache::BoundedCache<String, CachedMedia>>,
    pub dump_cdn: Arc<crate::cache::DumpCdnCache>,
    pub dump_manager: Arc<crate::cache::DumpChannelManager>,
    /// Shared HTTP client for lightweight diagnostic calls (/check, /status, /speedtest).
    /// Per-call timeouts are applied per-request via `.timeout(..)` on the builder, so a
    /// single connection pool + TLS state is reused instead of rebuilt on every command.
    pub http: reqwest::Client,
    pub topic_settings: crate::bot::topic_settings::TopicSettings,
}

impl AppState {
    pub async fn new(
        config: Config,
    ) -> anyhow::Result<(
        Arc<Self>,
        Receiver<grammers_session::updates::UpdatesLike>,
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
            .tcp_nodelay(true)
            .pool_max_idle_per_host(16)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let topic_settings = crate::bot::topic_settings::TopicSettings::load().await;

        let worker_pool = Arc::new(crate::pool::WorkerPool::new(config.worker_concurrency()));
        let bot_stats = BotStats::new(session.bot_username);
        bot_stats.set_total_workers(worker_pool.total_slots() as u64);

        let media_cache = Arc::new(crate::cache::BoundedCache::new(
            1000,
            std::time::Duration::from_secs(24 * 3600),
        ));

        let cdn_path =
            std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()) + "/dump_cdn.sqlite";
        let dump_cdn = Arc::new(crate::cache::DumpCdnCache::open(&cdn_path).await?);
        let dump_manager =
            Arc::new(crate::cache::DumpChannelManager::new(config.dump_channel_target()));

        if dump_manager.is_enabled() {
            let dm = dump_manager.clone();
            let cl = session.client.clone();
            tokio::spawn(async move {
                let _ = dm.get_peer(&cl).await;
            });
        }

        let state = Arc::new(Self {
            client: session.client,
            config,
            registry,
            bot_stats,
            worker_pool,
            singleflight: Arc::new(crate::pool::SingleFlight::new()),
            pending_downloads: Arc::new(dashmap::DashMap::new()),
            media_cache,
            dump_cdn,
            dump_manager,
            http,
            topic_settings,
        });

        Ok((state, session.updates_rx))
    }
}
