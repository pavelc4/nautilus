use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct SoundCloudSource {
    url: Option<String>,
    quality: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SoundCloudResponse {
    title: Option<String>,
    sources: Option<Vec<SoundCloudSource>>,
}

pub struct SoundCloudExtractor;

#[async_trait]
impl Extractor for SoundCloudExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("soundcloud.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let services = [
            "https://urlmp4.com/wp-json/aio-dl/video-data/",
            "https://soundcloudmp3.com/wp-json/aio-dl/video-data/",
            "https://scdownloader.com/wp-json/aio-dl/video-data/",
            "https://soundcloudto.com/wp-json/aio-dl/video-data/",
        ];

        let mut last_error: Option<anyhow::Error> = None;
        for service_url in &services {
            let resp = match client
                .post(*service_url)
                .form(&[("url", url)])
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(anyhow::anyhow!(e));
                    continue;
                }
            };
            let result: Result<SoundCloudResponse, _> = resp.json().await;
            let result = match result {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("JSON decode error: {}", e));
                    continue;
                }
            };

            let download_url = result
                .sources
                .and_then(|s| s.into_iter().find(|s| s.url.is_some()))
                .and_then(|s| s.url)
                .ok_or_else(|| anyhow::anyhow!("no audio URL"));
            let download_url = match download_url {
                Ok(url) => url,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };

            let title = result.title.as_deref().unwrap_or("track");
            let filename = format!("{title}.mp3");
            let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

            let meta = MediaMeta {
                filename,
                mime_type: "audio/mpeg".into(),
                size,
                duration_secs: None,
                dims: None,
                kind: MediaKind::Audio,
                title: result.title,
                description: None,
            };

            return Ok(vec![MediaItem { meta, reader }]);
        }

        Err(anyhow::anyhow!(
            "all SoundCloud download services failed: {}",
            last_error.map(|e| e.to_string()).unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}
