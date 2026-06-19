pub mod extractors;

use async_trait::async_trait;

use crate::provider::{MediaMeta, MediaReader, Provider};

#[async_trait]
pub trait Extractor: Send + Sync {
    fn can_handle(&self, url: &str) -> bool;
    async fn resolve(
        &self,
        url: &str,
    ) -> anyhow::Result<(MediaMeta, MediaReader)>;
}

pub struct ScraperProvider {
    extractors: Vec<Box<dyn Extractor>>,
}

impl ScraperProvider {
    pub fn new(extractors: Vec<Box<dyn Extractor>>) -> Self {
        Self { extractors }
    }
}

#[async_trait]
impl Provider for ScraperProvider {
    fn can_handle(&self, url: &str) -> bool {
        self.extractors.iter().any(|e| e.can_handle(url))
    }

    async fn resolve(
        &self,
        url: &str,
    ) -> anyhow::Result<(MediaMeta, MediaReader)> {
        let extractor = self
            .extractors
            .iter()
            .find(|e| e.can_handle(url))
            .ok_or_else(|| anyhow::anyhow!("unsupported URL: {url}"))?;
        extractor.resolve(url).await
    }
}
