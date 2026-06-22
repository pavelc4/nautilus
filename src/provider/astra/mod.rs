use async_trait::async_trait;

use crate::provider::{MediaItem, MediaKind, Provider};

pub mod client;
pub mod endpoint;
pub mod types;

pub use client::*;
pub use endpoint::AstraEndpoint;
pub use types::*;

pub struct AstraProvider {
    api_url: String,
    client: reqwest::Client,
}

impl AstraProvider {
    pub fn new(api_url: String) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300)) // 5 minutes timeout for downloading large media
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { api_url, client }
    }
}

#[async_trait]
impl Provider for AstraProvider {
    fn can_handle(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        AstraEndpoint::try_from(&parsed).is_ok()
    }

    async fn resolve(&self, url: &str, format: Option<&str>) -> anyhow::Result<Vec<MediaItem>> {
        let parsed_url = url::Url::parse(url)?;
        let endpoint = AstraEndpoint::try_from(&parsed_url)?;
        let platform = endpoint.platform();

        let mut api_url = url::Url::parse(&self.api_url)?;
        api_url.set_path(endpoint.path());
        api_url.query_pairs_mut().append_pair("url", url);

        let resp = self.client.get(api_url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Astra API returned error ({}): {}", status, err_text);
        }

        let api_resp: AstraResponse = resp.json().await?;
        if !api_resp.success {
            anyhow::bail!("Astra error: {}", api_resp.message);
        }

        let data = api_resp
            .data
            .ok_or_else(|| anyhow::anyhow!("Astra returned success but no data"))?;

        let title = data.title.clone().or_else(|| data.caption.clone());
        let description = if data.title.is_some() {
            data.caption.clone()
        } else {
            None
        };

        let mut items = Vec::new();

        if let Some(ref downloads) = data.downloads {
            // Find all kinds of downloads
            let video_items: Vec<_> = downloads
                .iter()
                .filter(|d| d.media_type == AstraMediaType::Video)
                .collect();

            let slideshow_items: Vec<_> = downloads
                .iter()
                .filter(|d| {
                    d.media_type == AstraMediaType::Image || d.media_type == AstraMediaType::Slide
                })
                .collect();

            let audio_items: Vec<_> = downloads
                .iter()
                .filter(|d| d.media_type == AstraMediaType::Audio)
                .collect();

            let mut format_to_use = format;
            if format_to_use.is_none() {
                if !video_items.is_empty() {
                    format_to_use = Some("video");
                } else if !slideshow_items.is_empty() {
                    format_to_use = Some("photo");
                } else if !audio_items.is_empty() {
                    format_to_use = Some("audio");
                }
            }

            match format_to_use {
                Some("video") if !video_items.is_empty() => {
                    let selected_video = match platform {
                        "youtube" => {
                            // Look for a combined format (label does not contain "no audio")
                            let combined_videos: Vec<_> = video_items
                                .iter()
                                .filter(|v| {
                                    let label = v.label.as_deref().unwrap_or("").to_lowercase();
                                    !label.contains("no audio")
                                })
                                .collect();

                            if !combined_videos.is_empty() {
                                // Find the one with highest quality
                                combined_videos
                                    .into_iter()
                                    .max_by_key(|v| {
                                        parse_quality(v.quality.as_deref().unwrap_or(""))
                                    })
                                    .copied()
                            } else {
                                // Fallback to highest quality overall video
                                video_items.into_iter().max_by_key(|v| {
                                    parse_quality(v.quality.as_deref().unwrap_or(""))
                                })
                            }
                        }
                        "tiktok" => {
                            // Look for No Watermark -> HD -> Original -> With Watermark -> first
                            video_items
                                .iter()
                                .find(|v| v.label.as_deref() == Some("No Watermark"))
                                .or_else(|| {
                                    video_items
                                        .iter()
                                        .find(|v| v.label.as_deref() == Some("HD"))
                                })
                                .or_else(|| {
                                    video_items
                                        .iter()
                                        .find(|v| v.label.as_deref() == Some("Original"))
                                })
                                .or_else(|| {
                                    video_items
                                        .iter()
                                        .find(|v| v.label.as_deref() == Some("With Watermark"))
                                })
                                .or_else(|| video_items.first())
                                .copied()
                        }
                        _ => {
                            // Pick highest quality or first
                            video_items
                                .into_iter()
                                .max_by_key(|v| parse_quality(v.quality.as_deref().unwrap_or("")))
                        }
                    };

                    if let Some(v) = selected_video {
                        let sanitized =
                            sanitize_filename(title.as_deref().unwrap_or("video"), "video");
                        let filename = format!("{sanitized}.mp4");
                        let media_item = download_item(
                            &self.client,
                            v.url.clone(),
                            MediaKind::Video,
                            "video/mp4".into(),
                            filename,
                            title.clone(),
                            description.clone(),
                        )
                        .await?;
                        items.push(media_item);
                    }
                }
                Some("photo") if !slideshow_items.is_empty() => {
                    for (idx, item) in slideshow_items.into_iter().enumerate() {
                        let sanitized =
                            sanitize_filename(title.as_deref().unwrap_or("image"), "image");
                        let filename = format!("{sanitized}_{idx}.jpg");
                        let media_item = download_item(
                            &self.client,
                            item.url.clone(),
                            MediaKind::Photo,
                            "image/jpeg".into(),
                            filename,
                            title.clone(),
                            description.clone(),
                        )
                        .await?;
                        items.push(media_item);
                    }
                }
                Some("audio") if !audio_items.is_empty() => {
                    if let Some(a) = audio_items.first() {
                        let sanitized =
                            sanitize_filename(title.as_deref().unwrap_or("audio"), "audio");
                        let filename = format!("{sanitized}.mp3");
                        let media_item = download_item(
                            &self.client,
                            a.url.clone(),
                            MediaKind::Audio,
                            "audio/mpeg".into(),
                            filename,
                            title.clone(),
                            description.clone(),
                        )
                        .await?;
                        items.push(media_item);
                    }
                }
                _ => {}
            }
        }

        // If no downloads items resolved but we have photos and videos array (Meta/Instagram/Facebook/Threads carousels)
        if items.is_empty() {
            let mut extracted_media = Vec::new();
            let wants_photo = format.is_none() || format == Some("photo");
            let wants_video = format.is_none() || format == Some("video");

            if wants_photo {
                for (idx, p) in data.photos.iter().flatten().enumerate() {
                    if let Some(u) = p.get_url() {
                        extracted_media.push((u, MediaKind::Photo, idx));
                    }
                }
            }

            if wants_video {
                for (idx, v) in data.videos.iter().flatten().enumerate() {
                    extracted_media.push((v.url.clone(), MediaKind::Video, idx));
                }
            }

            for (idx, (url, kind, segment_idx)) in extracted_media.into_iter().enumerate() {
                let ext = match kind {
                    MediaKind::Video => "mp4",
                    MediaKind::Photo => "jpg",
                    _ => "bin",
                };
                let mime_type = match kind {
                    MediaKind::Video => "video/mp4",
                    MediaKind::Photo => "image/jpeg",
                    _ => "application/octet-stream",
                };
                let default_base = match kind {
                    MediaKind::Video => "video",
                    MediaKind::Photo => "image",
                    _ => "media",
                };
                let sanitized =
                    sanitize_filename(title.as_deref().unwrap_or(default_base), default_base);
                let filename = format!("{sanitized}_{segment_idx}.{ext}");
                let media_item = download_item(
                    &self.client,
                    url,
                    kind,
                    mime_type.into(),
                    filename,
                    if idx == 0 { title.clone() } else { None },
                    if idx == 0 { description.clone() } else { None },
                )
                .await?;
                items.push(media_item);
            }
        }

        if items.is_empty() {
            anyhow::bail!("Astra resolved URL, but no downloadable media was found");
        }

        Ok(items)
    }

    async fn fetch_metadata(
        &self,
        url: &str,
    ) -> anyhow::Result<crate::provider::MediaMetadataInfo> {
        let parsed_url = url::Url::parse(url)?;
        let endpoint = AstraEndpoint::try_from(&parsed_url)?;

        let mut api_url = url::Url::parse(&self.api_url)?;
        api_url.set_path(endpoint.path());
        api_url.query_pairs_mut().append_pair("url", url);

        let resp = self.client.get(api_url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Astra API returned error ({}): {}", status, err_text);
        }

        let api_resp: AstraResponse = resp.json().await?;
        if !api_resp.success {
            anyhow::bail!("Astra error: {}", api_resp.message);
        }

        let data = api_resp
            .data
            .ok_or_else(|| anyhow::anyhow!("Astra returned success but no data"))?;

        let mut has_video = false;
        let mut has_photo = false;
        let mut has_audio = false;

        if let Some(ref downloads) = data.downloads {
            has_video = downloads
                .iter()
                .any(|d| d.media_type == AstraMediaType::Video);
            has_photo = downloads.iter().any(|d| {
                d.media_type == AstraMediaType::Image || d.media_type == AstraMediaType::Slide
            });
            has_audio = downloads
                .iter()
                .any(|d| d.media_type == AstraMediaType::Audio);
        }

        if let Some(ref photos) = data.photos {
            if !photos.is_empty() {
                has_photo = true;
            }
        }

        if let Some(ref videos) = data.videos {
            if !videos.is_empty() {
                has_video = true;
            }
        }

        Ok(crate::provider::MediaMetadataInfo {
            has_video,
            has_photo,
            has_audio,
        })
    }
}
