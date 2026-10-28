use std::time::Duration;
use nautilus::bot::style::*;

#[test]
fn test_html_escape() {
    assert_eq!(html_escape("a & b < c > \"d\""), "a &amp; b &lt; c &gt; &quot;d&quot;");
    assert_eq!(bold("hello"), "<b>hello</b>");
    assert_eq!(italic("world"), "<i>world</i>");
    assert_eq!(code("x = 1"), "<code>x = 1</code>");
}

#[test]
fn test_format_duration() {
    assert_eq!(format_duration(Duration::from_millis(500)), "500 ms");
    assert_eq!(format_duration(Duration::from_millis(1500)), "1.50s");
    assert_eq!(format_duration(Duration::from_secs(75)), "1m 15s");
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(500 * 1024), "0.49 MB");
    assert_eq!(format_bytes(10 * 1024 * 1024), "10.00 MB");
    assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GB");
}

#[test]
fn test_detect_platform_and_source_link() {
    assert_eq!(detect_platform("https://x.com/user/status/123"), Some("Twitter/X"));
    assert_eq!(detect_platform("https://pixiv.net/en/artworks/123"), Some("Pixiv"));
    assert_eq!(detect_platform("https://instagram.com/p/123"), Some("Instagram"));

    let link = get_source_link("https://pixiv.net/en/artworks/123");
    assert_eq!(link, "<a href=\"https://pixiv.net/en/artworks/123\">Pixiv</a>");
}

#[test]
fn test_build_media_caption() {
    let caption = build_media_caption(CaptionOptions {
        title: Some("My Art & Title"),
        description: Some("Artwork description"),
        source_url: "https://pixiv.net/en/artworks/123",
        type_str: "ZIP Archive",
        resolution: None,
        total_bytes: 5 * 1024 * 1024,
        duration: None,
        elapsed: Duration::from_millis(1200),
        cache_label: Some("cached"),
        media_hash: Some("7a8f9c0e1b2d3e4f5a6b7c8d9e0f1a2b"),
        sender_name: Some("<a href=\"https://t.me/Alice\">Alice</a>"),
        bot_username: Some("aetherstagingbot"),
    });

    assert!(caption.contains("<b>My Art &amp; Title</b>"));
    assert!(caption.contains("<blockquote>"));
    assert!(caption.contains("🔗 Source: <a href=\"https://pixiv.net/en/artworks/123\">Pixiv</a>"));
    assert!(caption.contains("🏷 Type: ZIP Archive"));
    assert!(caption.contains("💾 Size: 5.00 MB"));
    assert!(caption.contains("⏱️ Process Time: 1.20s (cached)"));
    assert!(caption.contains("🔑 Hash: <code>7a8f9c0e1b2d3e4f</code>"));
    assert!(caption.contains("👤 By: <a href=\"https://t.me/Alice\">Alice</a>"));
    assert!(caption.contains("🤖 Downloaded via @aetherstagingbot"));
}
