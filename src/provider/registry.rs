use crate::provider::{MediaItem, Provider};

pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Self {
        Self { providers }
    }

    pub fn resolve(&self, url: &str) -> anyhow::Result<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.can_handle(url))
            .map(|p| p.as_ref())
            .ok_or_else(|| anyhow::anyhow!("unsupported URL: {url}"))
    }

    pub async fn resolve_and_fetch(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let provider = self.resolve(url)?;
        provider.resolve(url).await
    }
}
