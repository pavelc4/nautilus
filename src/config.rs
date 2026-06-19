use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub telegram_app_id: i32,
    pub telegram_app_hash: String,
    pub bot_token: String,
    pub owner_id: i64,

    pub ytdlp_cookies: Option<String>,

    pub max_file_size_mb: Option<u64>,
    pub rate_limit_tokens: Option<u64>,
    pub rate_limit_refill_secs: Option<u64>,
    pub progress_edit_secs: Option<u64>,
    pub max_concurrent_jobs: Option<usize>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        Ok(envy::from_env::<Config>()?)
    }

    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb.unwrap_or(2000) * 1024 * 1024
    }

    pub fn rate_limit_tokens(&self) -> u64 {
        self.rate_limit_tokens.unwrap_or(3)
    }

    pub fn rate_limit_refill_secs(&self) -> u64 {
        self.rate_limit_refill_secs.unwrap_or(60)
    }

    pub fn progress_edit_secs(&self) -> u64 {
        self.progress_edit_secs.unwrap_or(3)
    }

    pub fn max_concurrent_jobs(&self) -> usize {
        self.max_concurrent_jobs
            .unwrap_or_else(|| num_cpus::get().clamp(1, 4))
    }
}
