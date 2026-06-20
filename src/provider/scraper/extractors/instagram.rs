use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta};

const MOBILE_UA: &str = "Instagram 347.0.0.30.101 Android (33/13; 540dpi; 1080x2400; samsung; SM-S908E; q5q; qcom; en_US; 679421354)";
const IG_APP_ID: &str = "936619743392459";
const API_BASE: &str = "https://i.instagram.com/api/v1";

#[derive(Deserialize)]
#[allow(dead_code)]
struct MediaVersion {
    url: String,
    width: i32,
    height: i32,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ImageCandidate {
    url: String,
    width: i32,
    height: i32,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ImageVersions {
    candidates: Vec<ImageCandidate>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CarouselItemData {
    media_type: i32,
    video_versions: Option<Vec<MediaVersion>>,
    image_versions2: Option<ImageVersions>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct CaptionData {
    text: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct UserData {
    username: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct MediaItemData {
    id: String,
    media_type: i32,
    video_versions: Option<Vec<MediaVersion>>,
    image_versions2: Option<ImageVersions>,
    carousel_media: Option<Vec<CarouselItemData>>,
    caption: Option<CaptionData>,
    user: Option<UserData>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct MediaInfoResponse {
    items: Vec<MediaItemData>,
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn shortcode_to_media_id(shortcode: &str) -> anyhow::Result<String> {
    let mut id: u64 = 0;
    for c in shortcode.chars() {
        let pos = BASE64_ALPHABET
            .iter()
            .position(|&a| a == c as u8)
            .ok_or_else(|| anyhow::anyhow!("invalid shortcode char: {c}"))?;
        id = id
            .checked_mul(64)
            .and_then(|v| v.checked_add(pos as u64))
            .ok_or_else(|| anyhow::anyhow!("media ID overflow"))?;
    }
    Ok(id.to_string())
}

fn extract_shortcode(url: &str) -> Option<&str> {
    let url = url.trim_end_matches('/');
    for prefix in &["/p/", "/reel/", "/reels/", "/tv/"] {
        if let Some(idx) = url.rfind(prefix) {
            let start = idx + prefix.len();
            let rest = &url[start..];
            let end = rest
                .find('/')
                .or_else(|| rest.find('?'))
                .unwrap_or(rest.len());
            return Some(&rest[..end]);
        }
    }
    None
}

pub struct InstagramExtractor {
    cookies: Option<String>,
}

impl InstagramExtractor {
    pub fn new(cookies: Option<String>) -> Self {
        Self { cookies }
    }

    fn api_headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("User-Agent", MOBILE_UA.parse().unwrap());
        h.insert("Accept", "application/json".parse().unwrap());
        h.insert("X-IG-App-ID", IG_APP_ID.parse().unwrap());
        if let Some(ref c) = self.cookies {
            h.insert("Cookie", c.parse().unwrap());
            for part in c.split(';') {
                let p = part.trim();
                if let Some(token) = p.strip_prefix("csrftoken=") {
                    h.insert("X-CSRFToken", token.parse().unwrap());
                }
            }
        }
        h
    }

    async fn build_item(
        download_url: String,
        is_video: bool,
        index: usize,
        title: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<MediaItem> {
        let kind = if is_video {
            MediaKind::Video
        } else {
            MediaKind::Photo
        };
        let ext = if is_video { "mp4" } else { "jpg" };

        let unique = &download_url[download_url.len().saturating_sub(16)..];
        let filename = format!("instagram_{index}_{unique}.{ext}");

        let (size, reader) =
            helpers::fetch_stream(&helpers::no_timeout_client(), &download_url).await?;

        let meta = MediaMeta {
            filename,
            mime_type: if is_video {
                "video/mp4"
            } else {
                "image/jpeg"
            }
            .into(),
            size,
            duration_secs: None,
            dims: None,
            kind,
            title,
            description,
        };

        Ok(MediaItem { meta, reader })
    }
}

#[async_trait]
impl Extractor for InstagramExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("instagram.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let shortcode =
            extract_shortcode(url).ok_or_else(|| anyhow::anyhow!("could not extract shortcode"))?;

        let media_id = shortcode_to_media_id(shortcode)?;

        let client = helpers::http_client();
        let api_url = format!("{API_BASE}/media/{media_id}/info/");
        let resp: MediaInfoResponse = client
            .get(&api_url)
            .headers(self.api_headers())
            .send()
            .await?
            .json()
            .await?;

        let item = resp
            .items
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("media not found"))?;

        let caption = item.caption.as_ref().map(|c| c.text.clone());
        let title = caption
            .as_ref()
            .and_then(|t| t.lines().next().map(|l| l.trim().to_string()));

        if item.media_type == 8 {
            let carousel = item
                .carousel_media
                .ok_or_else(|| anyhow::anyhow!("carousel media missing"))?;
            let mut items = Vec::with_capacity(carousel.len());
            for (i, cm) in carousel.into_iter().enumerate() {
                let (download_url, is_video) = if cm.media_type == 2 {
                    let url = cm
                        .video_versions
                        .as_ref()
                        .and_then(|v| v.first())
                        .map(|v| v.url.clone())
                        .ok_or_else(|| anyhow::anyhow!("no carousel video URL"))?;
                    (url, true)
                } else {
                    let url = cm
                        .image_versions2
                        .as_ref()
                        .and_then(|iv| iv.candidates.first())
                        .map(|c| c.url.clone())
                        .ok_or_else(|| anyhow::anyhow!("no carousel image URL"))?;
                    (url, false)
                };

                let item_title = if i == 0 {
                    title.clone()
                } else {
                    None
                };
                let item_desc = if i == 0 {
                    caption.clone()
                } else {
                    None
                };

                let mi = Self::build_item(download_url, is_video, i, item_title, item_desc).await?;
                items.push(mi);
            }
            Ok(items)
        } else {
            let (download_url, is_video) = if item.media_type == 2 {
                let url = item
                    .video_versions
                    .as_ref()
                    .and_then(|v| v.first())
                    .map(|v| v.url.clone())
                    .ok_or_else(|| anyhow::anyhow!("no video URL"))?;
                (url, true)
            } else {
                let url = item
                    .image_versions2
                    .as_ref()
                    .and_then(|iv| iv.candidates.first())
                    .map(|c| c.url.clone())
                    .ok_or_else(|| anyhow::anyhow!("no image URL"))?;
                (url, false)
            };

            let mi = Self::build_item(download_url, is_video, 0, title, caption).await?;
            Ok(vec![mi])
        }
    }
}
