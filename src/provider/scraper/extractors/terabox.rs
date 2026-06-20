use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

pub struct TeraboxExtractor;

#[async_trait]
impl Extractor for TeraboxExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("terabox.com/") || url.contains("1024tera.com/") || url.contains("teraboxapp.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let resp = client.get("https://teradownloaderz.com").send().await?;
        let html_text = resp.text().await?;
        let nonce = {
            let doc = Html::parse_document(&html_text);
            let sel = Selector::parse("input[name='nonce'], input#nonce, [data-nonce]")
                .map_err(|_| anyhow::anyhow!("bad selector"))?;
            doc.select(&sel)
                .filter_map(|el| el.value().attr("value").or(el.value().attr("data-nonce")))
                .next()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("no nonce found"))?
        };

        let api_resp = client
            .post("https://teradownloaderz.com/wp-admin/admin-ajax.php")
            .form(&[
                ("action", "teradownloader"),
                ("nonce", &nonce),
                ("url", url),
            ])
            .send()
            .await?;

        let body: serde_json::Value = api_resp.json().await?;
        let download_url = body
            .get("download_url")
            .or(body.get("url"))
            .or_else(|| body.get("data").and_then(|d: &serde_json::Value| d.get("url")))
            .and_then(|v: &serde_json::Value| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no download URL in response"))?;

        let filename = format!("terabox_{}.mp4", &download_url[download_url.len().saturating_sub(16)..]);
        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), download_url).await?;

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
