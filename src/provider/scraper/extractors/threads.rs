use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct ThreadsApiResponse {
    status: String,
    data: Option<String>,
    mess: Option<String>,
}

pub struct ThreadsExtractor;

#[async_trait]
impl Extractor for ThreadsExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("threads.net/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let resp: ThreadsApiResponse = client
            .post("https://lovethreads.net/api/ajaxSearch")
            .form(&[("q", url), ("lang", "en")])
            .send()
            .await?
            .json()
            .await?;

        let html_data = resp.data.ok_or_else(|| {
            let msg = resp.mess.unwrap_or_default();
            anyhow::anyhow!("threads: {msg}")
        })?;

        let download_url = {
            let doc = Html::parse_document(&html_data);
            let video_sel =
                Selector::parse("video source[src], video[src], a[href*='cdn']").map_err(|_| anyhow::anyhow!("bad selector"))?;
            let img_sel =
                Selector::parse("img[src*='cdn'], .post-media img").map_err(|_| anyhow::anyhow!("bad selector"))?;

            doc.select(&video_sel)
                .filter_map(|el| el.value().attr("src"))
                .next()
                .or_else(|| {
                    doc.select(&img_sel)
                        .filter_map(|el| el.value().attr("src"))
                        .next()
                })
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("no media found"))?
        };

        let is_video = download_url.contains(".mp4") || download_url.contains(".mov");
        let kind = if is_video { MediaKind::Video } else { MediaKind::Photo };
        let ext = if is_video { "mp4" } else { "jpg" };
        let filename = format!("threads_{}.{ext}", &download_url[download_url.len().saturating_sub(12)..]);
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: if is_video { "video/mp4" } else { "image/jpeg" }.into(),
            size,
            duration_secs: None,
            dims: None,
            kind,
            title: None,
            description: None,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
