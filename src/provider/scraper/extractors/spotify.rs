use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct SpotifyResponse {
    status: Option<String>,
    url: Option<String>,
    title: Option<String>,
    thumbnail: Option<String>,
}

pub struct SpotifyExtractor;

#[async_trait]
impl Extractor for SpotifyExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("open.spotify.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let resp: SpotifyResponse = client
            .post("https://musicfab.io/api/spotify")
            .json(&serde_json::json!({"url": url}))
            .send()
            .await?
            .json()
            .await?;

        let download_url = resp.url.ok_or_else(|| anyhow::anyhow!("no download URL"))?;
        let title = resp.title.as_deref().unwrap_or("track");
        let filename = format!("{title}.mp3");
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: "audio/mpeg".into(),
            size,
            duration_secs: None,
            dims: None,
            kind: MediaKind::Audio,
            title: resp.title,
            description: None,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
