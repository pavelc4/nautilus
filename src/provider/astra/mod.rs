use async_trait::async_trait;

use crate::provider::{MediaItem, MediaKind, MediaMeta, Provider};

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
            .tcp_nodelay(true)
            .pool_max_idle_per_host(16)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { api_url, client }
    }

    async fn fetch_data(&self, url: &str) -> anyhow::Result<AstraData> {
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

        api_resp
            .data
            .ok_or_else(|| anyhow::anyhow!("Astra returned success but no data"))
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

        let data = self.fetch_data(url).await?;

        let title = data.title.clone().or_else(|| data.caption.clone());
        let description = if data.title.is_some() {
            data.caption.clone()
        } else {
            None
        };

        let twitter_is_pure_video = platform == "twitter"
            && data.result_type.as_deref() == Some("video");

        let video_thumb = data.thumbnail.clone().or_else(|| {
            data.downloads.as_ref().and_then(|dl| {
                dl.iter()
                    .find(|d| d.media_type == AstraMediaType::Image)
                    .map(|d| d.url.clone())
            })
        });

        let mut items = Vec::new();

        let wants_video = format == Some("video")
            || format.is_none()
            || format == Some("both")
            || format == Some("all");
        let wants_photo = (format == Some("photo")
            || format.is_none()
            || format == Some("both")
            || format == Some("all"))
            && !twitter_is_pure_video;

        // `page:{idx}` downloads only the Nth image/slide in a multi-page artwork.
        let page_index = format.and_then(|f| f.strip_prefix("page:")).and_then(|n| n.parse::<usize>().ok());

        if let Some(mut downloads) = data.downloads {
            if let Some(idx) = page_index {
                downloads = downloads
                    .into_iter()
                    .enumerate()
                    .filter(|(i, d)| {
                        *i == idx
                            && (d.media_type == AstraMediaType::Image
                                || d.media_type == AstraMediaType::Slide)
                    })
                    .map(|(_, d)| d)
                    .collect();
            }
            let has_video_or_photo = downloads.iter().any(|d| {
                (d.media_type == AstraMediaType::Video
                    && !d.url.contains(".m3u8")
                    && !d.url.contains("hls_playlist"))
                    || d.media_type == AstraMediaType::Image
                    || d.media_type == AstraMediaType::Slide
            });
            let wants_audio = format == Some("audio") || (format.is_none() && !has_video_or_photo);

            // Per-page selection only ever yields a photo.
            let wants_photo = wants_photo || page_index.is_some();
            let wants_video = wants_video && page_index.is_none();

            if wants_video {
                let video_items: Vec<_> = downloads
                    .iter()
                    .filter(|d| {
                        d.media_type == AstraMediaType::Video
                            && !d.url.contains(".m3u8")
                            && !d.url.contains("hls_playlist")
                    })
                    .collect();
                if !video_items.is_empty() {
                    if platform == "twitter" {
                        let is_multi = video_items.len() > 1;
                        for (idx, v) in video_items.into_iter().enumerate() {
                            let sanitized = sanitize_filename(
                                title.as_deref().unwrap_or("video"),
                                "video",
                            );
                            let filename = if is_multi {
                                format!("{sanitized}_{idx}.mp4")
                            } else {
                                format!("{sanitized}.mp4")
                            };
                            let q = v.quality.as_deref().unwrap_or("");
                            let l = v.label.as_deref().unwrap_or("");
                            let dims = parse_dimensions(q, l, false);
                            let thumb = v.thumbnail.clone().or_else(|| video_thumb.clone());

                            let meta = MediaMeta {
                                filename,
                                mime_type: "video/mp4".into(),
                                size: 0,
                                duration_secs: None,
                                dims,
                                kind: MediaKind::Video,
                                title: title.clone(),
                                description: description.clone(),
                                thumbnail_url: thumb,
                            };
                            match download_item(&self.client, v.url.clone(), meta).await {
                                Ok(media_item) => items.push(media_item),
                                Err(e) => {
                                    if is_multi {
                                        tracing::warn!(idx, error = %e, "skipping video in multi-video tweet: download failed");
                                    } else {
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    } else {
                        let selected_video = match platform {
                            "youtube" => {
                                let combined_videos: Vec<_> = video_items
                                    .iter()
                                    .filter(|v| {
                                        let label = v.label.as_deref().unwrap_or("").to_lowercase();
                                        !label.contains("no audio")
                                    })
                                    .collect();

                                if !combined_videos.is_empty() {
                                    combined_videos
                                        .into_iter()
                                        .max_by_key(|v| {
                                            let q = v.quality.as_deref().unwrap_or("");
                                            let l = v.label.as_deref().unwrap_or("");
                                            parse_quality(q, l)
                                        })
                                        .copied()
                                } else {
                                    video_items.into_iter().max_by_key(|v| {
                                        let q = v.quality.as_deref().unwrap_or("");
                                        let l = v.label.as_deref().unwrap_or("");
                                        parse_quality(q, l)
                                    })
                                }
                            }
                            "tiktok" => video_items
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
                                .copied(),
                            _ => video_items.into_iter().max_by_key(|v| {
                                let q = v.quality.as_deref().unwrap_or("");
                                let l = v.label.as_deref().unwrap_or("");
                                parse_quality(q, l)
                            }),
                        };

                        if let Some(v) = selected_video {
                            let sanitized =
                                sanitize_filename(title.as_deref().unwrap_or("video"), "video");
                            let filename = format!("{sanitized}.mp4");
                            let q = v.quality.as_deref().unwrap_or("");
                            let l = v.label.as_deref().unwrap_or("");
                            let is_shorts = url.contains("/shorts/")
                                || url.contains("/reel/")
                                || url.contains("tiktok.com");
                            let dims = parse_dimensions(q, l, is_shorts);

                            let meta = MediaMeta {
                                filename,
                                mime_type: "video/mp4".into(),
                                size: 0,
                                duration_secs: None,
                                dims,
                                kind: MediaKind::Video,
                                title: title.clone(),
                                description: description.clone(),
                                thumbnail_url: video_thumb.clone(),
                            };
                            let media_item = download_item(&self.client, v.url.clone(), meta).await?;
                            items.push(media_item);
                        }
                    }
                }
            }

            if wants_photo {
                let slideshow_items: Vec<_> = downloads
                    .iter()
                    .filter(|d| {
                        d.media_type == AstraMediaType::Image
                            || d.media_type == AstraMediaType::Slide
                    })
                    .collect();
                for (idx, item) in slideshow_items.into_iter().enumerate() {
                    let sanitized = sanitize_filename(title.as_deref().unwrap_or("image"), "image");
                    let filename = format!("{sanitized}_{idx}.jpg");
                    let meta = MediaMeta {
                        filename,
                        mime_type: "image/jpeg".into(),
                        size: 0,
                        duration_secs: None,
                        dims: None,
                        kind: MediaKind::Photo,
                        title: title.clone(),
                        description: description.clone(),
                        thumbnail_url: None,
                    };
                    // Skip a photo whose CDN fetch fails (403 on a stale/oversized
                    // variant) rather than aborting the whole slideshow.
                    match download_item(&self.client, item.url.clone(), meta).await {
                        Ok(media_item) => items.push(media_item),
                        Err(e) => tracing::warn!(idx, error = %e, "skipping slideshow photo: CDN fetch failed"),
                    }
                }
            }

            if wants_audio {
                let audio_items: Vec<_> = downloads
                    .iter()
                    .filter(|d| {
                        d.media_type == AstraMediaType::Audio
                            && d.quality.as_deref() != Some("storyboard")
                            && !d
                                .label
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains("mhtml")
                    })
                    .collect();
                if let Some(a) = audio_items.first() {
                    let sanitized = sanitize_filename(title.as_deref().unwrap_or("audio"), "audio");
                    let filename = format!("{sanitized}.mp3");
                    let meta = MediaMeta {
                        filename,
                        mime_type: "audio/mpeg".into(),
                        size: 0,
                        duration_secs: None,
                        dims: None,
                        kind: MediaKind::Audio,
                        title: title.clone(),
                        description: description.clone(),
                        thumbnail_url: None,
                    };
                    let media_item = download_item(&self.client, a.url.clone(), meta).await?;
                    items.push(media_item);
                }
            }
        }

        // If no downloads items resolved but we have photos and videos array (Meta/Instagram/Facebook/Threads carousels)
        if items.is_empty() {
            let mut extracted_media = Vec::new();

            if wants_photo {
                for (idx, p) in data.photos.iter().flatten().enumerate() {
                    if let Some(u) = p.get_url() {
                        extracted_media.push((u, MediaKind::Photo, idx));
                    }
                }
            }

            if wants_video {
                // Meta returns a single video as quality variants (hd, sd) sharing
                // one poster; a carousel has distinct videos with distinct posters.
                // Keep only the best quality per poster so we send one best video
                // per real video, not hd+sd duplicates.
                for (idx, v) in
                    select_best_videos(data.videos.as_deref().unwrap_or(&[])).into_iter().enumerate()
                {
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
                let meta = MediaMeta {
                    filename,
                    mime_type: mime_type.into(),
                    size: 0,
                    duration_secs: None,
                    dims: None,
                    kind,
                    title: if idx == 0 { title.clone() } else { None },
                    description: if idx == 0 { description.clone() } else { None },
                    thumbnail_url: if kind == MediaKind::Video { data.thumbnail.clone() } else { None },
                };
                // Skip an item whose CDN fetch fails (e.g. FB returns 403 for a
                // stale/oversized variant) rather than aborting the whole album —
                // deliver the rest instead of failing the entire download.
                match download_item(&self.client, url, meta).await {
                    Ok(media_item) => items.push(media_item),
                    Err(e) => tracing::warn!(idx, error = %e, "skipping media item: CDN fetch failed"),
                }
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

        Ok(extract_metadata_info(platform, &data))
    }
}

pub fn extract_metadata_info(
    platform: &str,
    data: &AstraData,
) -> crate::provider::MediaMetadataInfo {
    let mut has_video = false;
    let mut has_photo = false;
    let mut has_audio = false;

    if let Some(ref downloads) = data.downloads {
        has_video = downloads.iter().any(|d| {
            d.media_type == AstraMediaType::Video
                && !d.url.contains(".m3u8")
                && !d.url.contains("hls_playlist")
        });
        has_photo = downloads.iter().any(|d| {
            d.media_type == AstraMediaType::Image || d.media_type == AstraMediaType::Slide
        });
        has_audio = downloads.iter().any(|d| {
            d.media_type == AstraMediaType::Audio
                && d.quality.as_deref() != Some("storyboard")
                && !d
                    .label
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("mhtml")
        });
    }

    if let Some(ref photos) = data.photos
        && !photos.is_empty()
    {
        has_photo = true;
    }

    if let Some(ref videos) = data.videos
        && !videos.is_empty()
    {
        has_video = true;
    }

    if data.result_type.as_deref() == Some("video") {
        has_video = true;
    }

    // For Twitter/X, if a tweet contains a video, any photo in downloads is just
    // the video's preview thumbnail — don't flag has_photo so user isn't prompted
    // with a bogus "Photo" / "Download All" button, unless the tweet has multiple/mixed media.
    if platform == "twitter" && has_video && data.result_type.as_deref() != Some("multiple") {
        has_photo = false;
    }

    crate::provider::MediaMetadataInfo {
        has_video,
        has_photo,
        has_audio,
    }
}

/// Collapse Meta video quality-variants down to the best one per poster.
///
/// A Facebook/Instagram single video comes back as several entries (hd, sd, …)
/// that all carry the same `thumbnail` (poster); a real carousel has distinct
/// videos with distinct posters. Group by poster and keep the highest-quality
/// entry per group — order-preserving — so the bot sends one best video per
/// real video instead of hd+sd duplicates. Entries with no thumbnail are treated
/// as distinct (kept as-is), so a source that omits posters is never over-merged.
pub fn select_best_videos(videos: &[AstraVideoItem]) -> Vec<&AstraVideoItem> {
    let mut groups: Vec<(Option<&str>, &AstraVideoItem)> = Vec::new();
    for v in videos {
        let key = v.thumbnail.as_deref().filter(|t| !t.is_empty());
        if let Some(k) = key
            && let Some(slot) = groups.iter_mut().find(|(gk, _)| *gk == Some(k))
        {
            let cur = parse_quality(slot.1.quality.as_deref().unwrap_or(""), "");
            let cand = parse_quality(v.quality.as_deref().unwrap_or(""), "");
            if cand > cur {
                slot.1 = v;
            }
            continue;
        }
        groups.push((key, v));
    }
    groups.into_iter().map(|(_, v)| v).collect()
}
