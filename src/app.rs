use std::sync::Arc;

use grammers_client::Client;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::bot::settings::SettingsMap;
use crate::bot::status::BotStats;
use crate::config::Config;
use crate::provider::registry::ProviderRegistry;
use crate::provider::scraper::ScraperProvider;
use crate::provider::scraper::extractors::{
    CapCutExtractor, InstagramExtractor, LinkedInExtractor, PinterestExtractor, RedditExtractor,
    SoundCloudExtractor, SpotifyExtractor, TeraboxExtractor, ThreadsExtractor, TikTokExtractor,
    TwitterExtractor,
};
use crate::provider::ytdlp::YtDlpProvider;

pub struct AppState {
    pub client: Client,
    pub config: Arc<Config>,
    pub registry: Arc<ProviderRegistry>,
    pub job_semaphore: Arc<Semaphore>,
    pub max_concurrent_jobs: usize,
    pub bot_stats: BotStats,
    pub settings: SettingsMap,
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

        let ytdlp = YtDlpProvider::new(config.ytdlp_cookies.clone());

        let scraper = ScraperProvider::new(vec![
            Box::new(TikTokExtractor),
            Box::new(InstagramExtractor::new(config.instagram_cookies.clone())),
            Box::new(TwitterExtractor),
            Box::new(ThreadsExtractor),
            Box::new(RedditExtractor),
            Box::new(PinterestExtractor),
            Box::new(TeraboxExtractor),
            Box::new(SpotifyExtractor),
            Box::new(SoundCloudExtractor),
            Box::new(CapCutExtractor),
            Box::new(LinkedInExtractor),
        ]);

        let registry = Arc::new(ProviderRegistry::new(vec![
            Box::new(ytdlp),
            Box::new(scraper),
        ]));

        let max_jobs = config.max_concurrent_jobs();
        let job_semaphore = Arc::new(Semaphore::new(max_jobs));

        tracing::info!(
            "app initialized: max_jobs={max_jobs}, max_file_size={}",
            config.max_file_size_bytes()
        );

        let state = Arc::new(Self {
            client: session.client,
            config,
            registry,
            job_semaphore,
            max_concurrent_jobs: max_jobs,
            bot_stats: BotStats::new(session.bot_username, session.bot_id),
            settings: SettingsMap::new(),
        });

        Ok((state, session.updates_rx))
    }
}
