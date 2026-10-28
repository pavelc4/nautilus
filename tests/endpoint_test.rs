use nautilus::provider::astra::endpoint::AstraEndpoint;
use url::Url;

#[test]
fn test_youtube_routing() {
    let urls = [
        "https://youtube.com/watch?v=123",
        "https://www.youtube.com/watch?v=123",
        "https://youtu.be/123",
        "https://yewtu.be/watch?v=123",
        "https://inv.nadeko.net/watch?v=123",
    ];
    for u in urls {
        let parsed = Url::parse(u).unwrap();
        assert_eq!(
            AstraEndpoint::try_from(&parsed).unwrap(),
            AstraEndpoint::YoutubeDownload
        );
    }
}

#[test]
fn test_tiktok_routing() {
    let download_url = Url::parse("https://www.tiktok.com/@user/video/123").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&download_url).unwrap(),
        AstraEndpoint::TiktokDownload
    );

    let music_url = Url::parse("https://www.tiktok.com/music/some-track-123").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&music_url).unwrap(),
        AstraEndpoint::TiktokMusic
    );

    let profile_url = Url::parse("https://www.tiktok.com/@username").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&profile_url).unwrap(),
        AstraEndpoint::TiktokProfile
    );
}

#[test]
fn test_twitter_x_routing() {
    let urls = [
        "https://twitter.com/status/123",
        "https://x.com/status/123",
        "https://t.co/status/123",
        "https://sub.x.com/status/123",
    ];
    for u in urls {
        let parsed = Url::parse(u).unwrap();
        assert_eq!(
            AstraEndpoint::try_from(&parsed).unwrap(),
            AstraEndpoint::TwitterDownload
        );
    }

    // Make sure a domain containing t.co but not ending/matching does not resolve to Twitter
    let reddit_fake = Url::parse("https://reddit.com/r/something").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&reddit_fake).unwrap(),
        AstraEndpoint::RedditDownload
    );
}

#[test]
fn test_instagram_routing() {
    let post_url = Url::parse("https://www.instagram.com/p/123").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&post_url).unwrap(),
        AstraEndpoint::InstagramDownload
    );

    let story_url = Url::parse("https://www.instagram.com/stories/user/123").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&story_url).unwrap(),
        AstraEndpoint::InstagramStories
    );

    let profile_url = Url::parse("https://www.instagram.com/username").unwrap();
    assert_eq!(
        AstraEndpoint::try_from(&profile_url).unwrap(),
        AstraEndpoint::InstagramProfile
    );
}

#[test]
fn test_pixiv_routing() {
    let urls = [
        "https://www.pixiv.net/artworks/145960605",
        "https://pixiv.net/en/artworks/145960605",
        "https://www.pixiv.net/users/18818121",
    ];
    for u in urls {
        let parsed = Url::parse(u).unwrap();
        assert_eq!(
            AstraEndpoint::try_from(&parsed).unwrap(),
            AstraEndpoint::PixivDownload
        );
    }
}
