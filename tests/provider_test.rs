use nautilus::provider::astra::select_best_videos;
use nautilus::provider::astra::types::{AstraData, AstraDownloadItem, AstraMediaType, AstraVideoItem};
use nautilus::provider::astra::extract_metadata_info;

fn vid(url: &str, quality: &str, thumb: Option<&str>) -> AstraVideoItem {
    AstraVideoItem {
        url: url.to_string(),
        quality: Some(quality.to_string()),
        thumbnail: thumb.map(str::to_string),
    }
}

#[test]
fn test_single_video_variants_collapse_to_best() {
    // hd + sd of ONE video (same poster) -> keep hd only.
    let vids = [
        vid("https://cdn/hd.mp4", "hd", Some("poster")),
        vid("https://cdn/sd.mp4", "sd", Some("poster")),
    ];
    let best = select_best_videos(&vids);
    assert_eq!(best.len(), 1);
    assert_eq!(best[0].url, "https://cdn/hd.mp4");
}

#[test]
fn test_distinct_posters_are_kept() {
    // A real carousel: two different videos with different posters -> keep both.
    let vids = [
        vid("https://cdn/a.mp4", "hd", Some("posterA")),
        vid("https://cdn/b.mp4", "hd", Some("posterB")),
    ];
    assert_eq!(select_best_videos(&vids).len(), 2);
}

#[test]
fn test_missing_thumbnail_never_over_merges() {
    // No posters -> treat each as distinct rather than collapsing to one.
    let vids = [
        vid("https://cdn/a.mp4", "hd", None),
        vid("https://cdn/b.mp4", "sd", None),
    ];
    assert_eq!(select_best_videos(&vids).len(), 2);
}

#[test]
fn test_twitter_video_omits_photo_choice_when_thumbnail_in_downloads() {
    let data = AstraData {
        result_type: Some("video".to_string()),
        title: Some("Twitter video test".to_string()),
        caption: None,
        thumbnail: Some("https://pbs.twimg.com/amplify_video_thumb/123/img/thumb.jpg".to_string()),
        downloads: Some(vec![
            AstraDownloadItem {
                label: Some("Photo".to_string()),
                url: "https://dl.snapcdn.app/get?token=thumb".to_string(),
                media_type: AstraMediaType::Image,
                quality: Some("original".to_string()),
                thumbnail: None,
            },
            AstraDownloadItem {
                label: Some("Video 1280p".to_string()),
                url: "https://dl.snapcdn.app/get?token=video".to_string(),
                media_type: AstraMediaType::Video,
                quality: Some("1280p".to_string()),
                thumbnail: None,
            },
        ]),
        photos: None,
        videos: None,
    };

    let meta = extract_metadata_info("twitter", &data);
    assert!(meta.has_video);
    assert!(!meta.has_photo, "thumbnail in downloads must not be counted as photo for twitter video");
    assert!(!meta.has_audio);
}

#[test]
fn test_twitter_photo_only_preserves_has_photo() {
    let data = AstraData {
        result_type: Some("photo".to_string()),
        title: Some("Twitter photo test".to_string()),
        caption: None,
        thumbnail: None,
        downloads: Some(vec![AstraDownloadItem {
            label: Some("Photo".to_string()),
            url: "https://dl.snapcdn.app/get?token=photo1".to_string(),
            media_type: AstraMediaType::Image,
            quality: Some("original".to_string()),
            thumbnail: None,
        }]),
        photos: None,
        videos: None,
    };

    let meta = extract_metadata_info("twitter", &data);
    assert!(!meta.has_video);
    assert!(meta.has_photo);
    assert!(!meta.has_audio);
}

#[test]
fn test_twitter_mixed_preserves_both_video_and_photo() {
    let data = AstraData {
        result_type: Some("multiple".to_string()),
        title: Some("Twitter mixed test".to_string()),
        caption: None,
        thumbnail: Some("https://pbs.twimg.com/media/HVmX1qRaUAAzer7.jpg".to_string()),
        downloads: Some(vec![
            AstraDownloadItem {
                label: Some("Photo".to_string()),
                url: "https://dl.snapcdn.app/get?token=photo1".to_string(),
                media_type: AstraMediaType::Image,
                quality: Some("original".to_string()),
                thumbnail: Some("https://pbs.twimg.com/media/HVmX1qRaUAAzer7.jpg".to_string()),
            },
            AstraDownloadItem {
                label: Some("Video 720p".to_string()),
                url: "https://dl.snapcdn.app/get?token=video1".to_string(),
                media_type: AstraMediaType::Video,
                quality: Some("720p".to_string()),
                thumbnail: Some("https://pbs.twimg.com/media/HVmX1qRaUAAzer7.jpg".to_string()),
            },
        ]),
        photos: None,
        videos: None,
    };

    let meta = extract_metadata_info("twitter", &data);
    assert!(meta.has_video, "mixed media tweet must have has_video = true");
    assert!(meta.has_photo, "mixed media tweet must have has_photo = true");
    assert!(!meta.has_audio);
}

#[test]
fn test_metadata_photo_count() {
    let mut items = Vec::new();
    for i in 0..71 {
        items.push(AstraDownloadItem {
            label: Some(format!("Page {}", i + 1)),
            url: format!("https://cdn/p{}.png", i),
            media_type: AstraMediaType::Image,
            quality: Some("original".to_string()),
            thumbnail: None,
        });
    }
    let data = AstraData {
        result_type: Some("photo".to_string()),
        title: Some("Manga 71 pages".to_string()),
        caption: None,
        thumbnail: None,
        downloads: Some(items),
        photos: None,
        videos: None,
    };

    let meta = extract_metadata_info("pixiv", &data);
    assert!(meta.has_photo);
    assert_eq!(meta.photo_count, 71);
}

#[test]
fn test_dynamic_download_concurrency() {
    use nautilus::provider::astra::dynamic_download_concurrency;

    let c = dynamic_download_concurrency();
    assert!(c >= 2 && c <= 16, "concurrency must be between 2 and 16");

    // Test env override
    unsafe {
        std::env::set_var("DOWNLOAD_CONCURRENCY", "8");
    }
    assert_eq!(dynamic_download_concurrency(), 8);

    unsafe {
        std::env::remove_var("DOWNLOAD_CONCURRENCY");
    }
}
