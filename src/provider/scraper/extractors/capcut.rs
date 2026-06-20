use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct CapCutResponse {
    url: Option<String>,
    download_url: Option<String>,
    data: Option<CapCutData>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CapCutData {
    url: Option<String>,
}

pub struct CapCutExtractor;

#[async_trait]
impl Extractor for CapCutExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("capcut.com/") || url.contains("capcut.net/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let resp: CapCutResponse = client
            .post("https://genviral.io/api/tools/social-downloader")
            .json(&serde_json::json!({"url": url, "platform": "capcut"}))
            .send()
            .await?
            .json()
            .await?;

        let download_url = resp
            .download_url
            .or(resp.url)
            .or(resp.data.and_then(|d| d.url))
            .ok_or_else(|| anyhow::anyhow!("no download URL"))?;

        let filename = format!("capcut_{}.mp4", &download_url[download_url.len().saturating_sub(16)..]);
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: "video/mp4".into(),
            size,
            duration_secs: None,
            dims: None,
            kind: MediaKind::Video,
            title: None,
            description: None,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
