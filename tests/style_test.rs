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
    assert_eq!(detect_platform("https://x.com/user/status/123"), Some("X"));
    assert_eq!(detect_platform("https://pixiv.net/en/artworks/123"), Some("Pixiv"));
    assert_eq!(detect_platform("https://instagram.com/p/123"), Some("Instagram"));

    let link = get_source_link("https://pixiv.net/en/artworks/123");
    assert_eq!(link, "<a href=\"https://pixiv.net/en/artworks/123\">Pixiv</a>");
}

#[test]
fn test_build_media_caption() {
    let res = build_media_caption(CaptionOptions {
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

    assert!(res.separate_text.is_none());
    let caption = res.caption;
    assert!(caption.contains("<b>My Art &amp; Title</b>"));
    assert!(caption.contains("<blockquote>"));
    assert!(caption.contains("📎 <b>Source:</b> <a href=\"https://pixiv.net/en/artworks/123\">Pixiv</a>"));
    assert!(caption.contains("📄 <b>Type:</b> ZIP Archive"));
    assert!(caption.contains("📁 <b>Size:</b> 5.00 MB"));
    assert!(caption.contains("⏱ <b>Process Time:</b> 1.20s <code>cached</code>"));
    assert!(caption.contains("🔑 <b>Hash:</b> <code>7a8f9c0e1b2d3e4f</code>"));
    assert!(caption.contains("👤 <b>By:</b> <a href=\"https://t.me/Alice\">Alice</a>"));
    assert!(caption.contains("🤖 Downloaded via @aetherstagingbot"));
    assert!(caption.contains("💫 Powered by <a href=\"https://github.com/pavelc4/astra.git\">Astra</a>"));
}

#[test]
fn test_build_media_caption_long_split() {
    let long_desc = "x".repeat(1200);
    let res = build_media_caption(CaptionOptions {
        title: Some("Very Long Caption"),
        description: Some(&long_desc),
        source_url: "https://x.com/user/status/123",
        type_str: "Photo",
        resolution: None,
        total_bytes: 1024,
        duration: None,
        elapsed: Duration::from_millis(500),
        cache_label: None,
        media_hash: None,
        sender_name: Some("Alice"),
        bot_username: Some("mybot"),
    });

    // When total caption exceeds 1000 characters, description is sent separately
    assert!(res.separate_text.is_some());
    assert!(!res.caption.contains("<blockquote>"));
    assert!(res.caption.contains("📎 <b>Source:</b>"));
    assert!(res.caption.contains("📁 <b>Size:</b>"));

    let sep = res.separate_text.unwrap();
    assert!(sep.contains("<b>Very Long Caption</b>"));
    assert!(sep.contains(&long_desc));
}
