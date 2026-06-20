use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
struct TwitterApiResponse {
    status: String,
    data: String,
}

pub struct TwitterExtractor;

#[async_trait]
impl Extractor for TwitterExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("twitter.com/") || url.contains("x.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let api_client = helpers::http_client();
        let resp: TwitterApiResponse = api_client
            .post("https://savetwitter.net/api/ajaxSearch")
            .form(&[("q", url), ("t", "media"), ("lang", "en")])
            .send()
            .await?
            .json()
            .await?;

        let (download_url, title) = {
            let doc = Html::parse_document(&resp.data);

            let title = Selector::parse("h3")
                .ok()
                .and_then(|sel| doc.select(&sel).next())
                .map(|el| {
                    el.text()
                        .collect::<Vec<_>>()
                        .concat()
                        .trim()
                        .to_string()
                });

            let dl_sel = Selector::parse("a.downloadbtn, .download-items a, a[href*='snapcdn']")
                .map_err(|_| anyhow::anyhow!("invalid selector"))?;
            let url = doc
                .select(&dl_sel)
                .filter_map(|a| a.value().attr("href"))
                .find(|href| href.contains("snapcdn.app"))
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("no snapcdn download URL found"))?;

            (url, title)
        };

        let filename = format!("twitter_{}.mp4", &download_url[download_url.len().saturating_sub(16)..]);
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: "video/mp4".into(),
            size,
            duration_secs: None,
            dims: None,
            kind: MediaKind::Video,
            title,
            description: None,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
