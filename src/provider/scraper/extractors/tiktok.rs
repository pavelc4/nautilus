use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

#[derive(Deserialize)]
#[allow(dead_code)]
struct TikwmResponse<T> {
    code: i32,
    msg: String,
    data: Option<T>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct TikTokPost {
    id: Option<String>,
    video_id: Option<String>,
    title: Option<String>,
    cover: Option<String>,
    play: Option<String>,
    wmplay: Option<String>,
    hdplay: Option<String>,
    music: Option<String>,
    images: Option<Vec<String>>,
    size: Option<i64>,
    hd_size: Option<i64>,
    author: Option<TikTokAuthor>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct TikTokAuthor {
    #[serde(rename = "uniqueId")]
    unique_id: Option<String>,
    nickname: Option<String>,
    avatar: Option<String>,
}

pub struct TikTokExtractor;

#[async_trait]
impl Extractor for TikTokExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("tiktok.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let api_url = "https://tikwm.com/api/";
        let resp = client
            .get(api_url)
            .query(&[("url", url), ("hd", "1")])
            .send()
            .await?;

        let tw: TikwmResponse<TikTokPost> = resp.json().await?;
        let data = tw
            .data
            .ok_or_else(|| anyhow::anyhow!("tikwm: {}", tw.msg))?;

        let prefix_id = data.id.as_deref().or(data.video_id.as_deref()).unwrap_or("post").to_string();

        // Slideshow (multiple images)
        if let Some(images) = &data.images {
            if !images.is_empty() {
                let mut items = Vec::with_capacity(images.len());
                for (i, img_url) in images.iter().enumerate() {
                    if img_url.is_empty() {
                        continue;
                    }
                    let filename = format!("tiktok_{}_{}.jpg", prefix_id, i + 1);
                    let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), img_url).await?;
                    let meta = MediaMeta {
                        filename,
                        mime_type: "image/jpeg".into(),
                        size,
                        duration_secs: None,
                        dims: None,
                        kind: MediaKind::Photo,
                        title: if i == 0 { data.title.clone() } else { None },
                        description: None,
                    };
                    items.push(MediaItem { meta, reader });
                }
                if !items.is_empty() {
                    return Ok(items);
                }
            }
        }

        // Single video
        let download_url = data
            .hdplay
            .as_deref()
            .or(data.play.as_deref())
            .or(data.wmplay.as_deref())
            .ok_or_else(|| anyhow::anyhow!("no video URL"))?;

        let filename = format!("tiktok_{prefix_id}.mp4");

        let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: "video/mp4".into(),
            size,
            duration_secs: None,
            dims: None,
            kind: MediaKind::Video,
            title: data.title,
            description: None,
        };

        Ok(vec![MediaItem { meta, reader }])
    }
}
