use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct LinkedInResponse {
    url: Option<String>,
    title: Option<String>,
    description: Option<String>,
    thumbnail: Option<String>,
}

pub struct LinkedInExtractor;

#[async_trait]
impl Extractor for LinkedInExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("linkedin.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let resp: LinkedInResponse = client
            .post("https://saywhat.ai/api/fetch-linkedin-page/")
            .json(&serde_json::json!({"url": url}))
            .send()
            .await?
            .json()
            .await?;

        let download_url = resp.url.ok_or_else(|| anyhow::anyhow!("no download URL"))?;
        let title = resp.title.as_deref().unwrap_or("video");
        let filename = format!("linkedin_{title}.mp4");
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: "video/mp4".into(),
            size,
            duration_secs: None,
            dims: None,
            kind: MediaKind::Video,
            title: resp.title,
            description: resp.description,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
