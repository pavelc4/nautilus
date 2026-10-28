use super::client::parse_quality;
use super::types::{AstraData, AstraMediaType, AstraVideoItem};

pub fn extract_metadata_info(
    platform: &str,
    data: &AstraData,
) -> crate::provider::MediaMetadataInfo {
    let mut has_video = false;
    let mut has_photo = false;
    let mut has_audio = false;
    let mut photo_count = 0;

    if let Some(ref downloads) = data.downloads {
        has_video = downloads.iter().any(|d| {
            d.media_type == AstraMediaType::Video
                && !d.url.contains(".m3u8")
                && !d.url.contains("hls_playlist")
        });
        let img_count = downloads
            .iter()
            .filter(|d| {
                d.media_type == AstraMediaType::Image || d.media_type == AstraMediaType::Slide
            })
            .count();
        if img_count > 0 {
            has_photo = true;
            photo_count += img_count;
        }
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
        photo_count += photos.len();
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
        photo_count = 0;
    }

    if has_photo && photo_count == 0 {
        photo_count = 1;
    }

    crate::provider::MediaMetadataInfo {
        has_video,
        has_photo,
        has_audio,
        photo_count,
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
