use nautilus::config::Config;
use nautilus::provider::astra::client::detect_image_dimensions;
use nautilus::provider::astra::endpoint::AstraEndpoint;
use nautilus::provider::{MediaKind, MediaMeta};
use std::borrow::Cow;
use url::Url;

#[test]
fn test_library_exports_and_models() {
    let meta = MediaMeta {
        filename: "test.mp4".to_string(),
        mime_type: Cow::Borrowed("video/mp4"),
        size: 1024,
        duration_secs: Some(10),
        dims: Some((1280, 720)),
        kind: MediaKind::Video,
        title: Some("Test Video".to_string()),
        description: Some("Test Description".to_string()),
        thumbnail_url: Some("https://example.com/thumb.jpg".to_string()),
    };

    assert_eq!(meta.filename, "test.mp4");
    assert_eq!(meta.kind, MediaKind::Video);
    assert_eq!(meta.size, 1024);
    assert_eq!(meta.dims, Some((1280, 720)));
}

#[test]
fn test_config_methods_and_defaults() {
    let cfg = Config {
        telegram_app_id: 12345,
        telegram_app_hash: "hash".to_string(),
        bot_token: "token".to_string(),
        owner_id: 999,
        astra_api_url: None,
        max_file_size_mb: None,
        progress_edit_secs: None,
        worker_concurrency: None,
    };

    assert_eq!(cfg.max_file_size_bytes(), 2000 * 1024 * 1024);
    assert_eq!(cfg.progress_edit_secs(), 3);
    assert!(cfg.worker_concurrency() >= 4);
}

#[test]
fn test_endpoint_resolution_for_all_supported_platforms() {
    let cases = [
        ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", AstraEndpoint::YoutubeDownload),
        ("https://youtu.be/dQw4w9WgXcQ", AstraEndpoint::YoutubeDownload),
        ("https://www.tiktok.com/@user/video/123456", AstraEndpoint::TiktokDownload),
        ("https://x.com/user/status/123456", AstraEndpoint::TwitterDownload),
        ("https://twitter.com/user/status/123456", AstraEndpoint::TwitterDownload),
        ("https://www.instagram.com/p/Cxyz123/", AstraEndpoint::InstagramDownload),
        ("https://www.facebook.com/watch/?v=123", AstraEndpoint::FacebookDownload),
        ("https://www.reddit.com/r/rust/comments/123", AstraEndpoint::RedditDownload),
        ("https://www.pinterest.com/pin/123", AstraEndpoint::PinterestDownload),
        ("https://www.pixiv.net/en/artworks/150811369", AstraEndpoint::PixivDownload),
    ];

    for (url_str, expected) in cases {
        let parsed = Url::parse(url_str).expect("valid test URL");
        let endpoint = AstraEndpoint::try_from(&parsed).expect("must resolve endpoint");
        assert_eq!(endpoint, expected, "Failed for URL: {url_str}");
    }
}

#[test]
fn test_image_dimension_detectors_integration() {
    // 1x1 8-bit PNG
    let png_header = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
        0x00, 0x00, 0x00, 0x0D, // IHDR chunk length (13)
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x04, 0x00, // width: 1024
        0x00, 0x00, 0x03, 0x00, // height: 768
        0x08, 0x06, 0x00, 0x00, 0x00,
    ];
    assert_eq!(detect_image_dimensions(&png_header), Some((1024, 768)));
}
