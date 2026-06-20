use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

pub struct PinterestExtractor;

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0);
            let lo = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0);
            out.push(char::from((hi * 16 + lo) as u8));
        } else {
            out.push(c);
        }
    }
    out
}

fn extract_url_from_force(href: &str) -> Option<String> {
    if !href.contains("url=") {
        return None;
    }
    let parts: Vec<&str> = href.splitn(2, "url=").collect();
    if parts.len() < 2 {
        return None;
    }
    let encoded = parts[1].trim();
    let encoded = encoded.split('#').next().unwrap_or(encoded);
    Some(percent_decode(encoded))
}

#[async_trait]
impl Extractor for PinterestExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("pinterest.com/") || url.contains("pin.it/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let page_url = format!("https://www.savepin.app/download.php?url={url}&lang=en&type=redirect");

        let html_text = client
            .get(&page_url)
            .header("Referer", "https://www.savepin.app/")
            .send()
            .await?
            .text()
            .await?;

        let download_url = {
            let doc = Html::parse_document(&html_text);
            let sel = Selector::parse("tbody tr a")
                .map_err(|_| anyhow::anyhow!("bad selector"))?;
            let from_rows = doc
                .select(&sel)
                .filter_map(|a| a.value().attr("href"))
                .find_map(extract_url_from_force);
            from_rows.or_else(|| {
                let fallback_sel = Selector::parse("a[href*='pinimg.com'], a[href*='.mp4'], a[href*='.jpg']").ok()?;
                doc.select(&fallback_sel)
                    .filter_map(|a| a.value().attr("href"))
                    .find(|href| href.contains("pinimg.com") || href.contains(".mp4"))
                    .map(|s| helpers::resolve_url(s, "https://www.savepin.app"))
            })
        }
        .ok_or_else(|| anyhow::anyhow!("no download URL"))?;

        let is_video = download_url.contains(".mp4");
        let kind = if is_video { MediaKind::Video } else { MediaKind::Photo };
        let ext = if is_video { "mp4" } else { "jpg" };
        let filename = format!("pinterest_{}.{ext}", &download_url[download_url.len().saturating_sub(16)..]);

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