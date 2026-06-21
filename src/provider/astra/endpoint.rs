#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstraEndpoint {
    YoutubeDownload,
    TiktokDownload,
    TiktokMusic,
    TiktokProfile,
    TwitterDownload,
    InstagramDownload,
    InstagramStories,
    InstagramProfile,
    FacebookDownload,
    ThreadsDownload,
    RedditDownload,
    PinterestDownload,
    TeraboxDownload,
    SpotifyDownload,
    SoundcloudDownload,
    CapcutDownload,
    LinkedinDownload,
}

impl AstraEndpoint {
    pub fn path(&self) -> &'static str {
        match self {
            Self::YoutubeDownload => "/api/youtube/download",
            Self::TiktokDownload => "/api/tiktok/download",
            Self::TiktokMusic => "/api/tiktok/music",
            Self::TiktokProfile => "/api/tiktok/profile",
            Self::TwitterDownload => "/api/twitter/download",
            Self::InstagramDownload => "/api/meta/instagram/download",
            Self::InstagramStories => "/api/meta/instagram/stories",
            Self::InstagramProfile => "/api/meta/instagram/profile",
            Self::FacebookDownload => "/api/meta/facebook/download",
            Self::ThreadsDownload => "/api/meta/threads/download",
            Self::RedditDownload => "/api/reddit/download",
            Self::PinterestDownload => "/api/pinterest/download",
            Self::TeraboxDownload => "/api/terabox/download",
            Self::SpotifyDownload => "/api/spotify/download",
            Self::SoundcloudDownload => "/api/soundcloud/download",
            Self::CapcutDownload => "/api/capcut/download",
            Self::LinkedinDownload => "/api/linkedin/download",
        }
    }

    pub fn platform(&self) -> &'static str {
        match self {
            Self::YoutubeDownload => "youtube",
            Self::TiktokDownload => "tiktok",
            Self::TiktokMusic => "tiktok_music",
            Self::TiktokProfile => "tiktok_profile",
            Self::TwitterDownload => "twitter",
            Self::InstagramDownload => "instagram",
            Self::InstagramStories => "instagram_stories",
            Self::InstagramProfile => "instagram_profile",
            Self::FacebookDownload => "facebook",
            Self::ThreadsDownload => "threads",
            Self::RedditDownload => "reddit",
            Self::PinterestDownload => "pinterest",
            Self::TeraboxDownload => "terabox",
            Self::SpotifyDownload => "spotify",
            Self::SoundcloudDownload => "soundcloud",
            Self::CapcutDownload => "capcut",
            Self::LinkedinDownload => "linkedin",
        }
    }
}

impl TryFrom<&url::Url> for AstraEndpoint {
    type Error = anyhow::Error;

    fn try_from(url: &url::Url) -> Result<Self, Self::Error> {
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("no host in URL"))?
            .to_lowercase();
        let path = url.path().to_lowercase();

        match host.as_str() {
            h if h.contains("youtube.com")
                || h.contains("youtu.be")
                || h.contains("yewtu.be")
                || h.contains("inv.nadeko.net") =>
            {
                Ok(Self::YoutubeDownload)
            }
            h if h.contains("tiktok.com") && path.contains("/music/") => Ok(Self::TiktokMusic),
            h if h.contains("tiktok.com")
                && path.contains("/@")
                && !path.contains("/video/")
                && !path.contains("/photo/") =>
            {
                Ok(Self::TiktokProfile)
            }
            h if h.contains("tiktok.com") => Ok(Self::TiktokDownload),
            h if h == "twitter.com"
                || h.ends_with(".twitter.com")
                || h == "x.com"
                || h.ends_with(".x.com")
                || h == "t.co"
                || h.ends_with(".t.co") =>
            {
                Ok(Self::TwitterDownload)
            }
            h if (h.contains("instagram.com") || h.contains("instagr.am"))
                && (path.contains("/stories/") || path.contains("/story/")) =>
            {
                Ok(Self::InstagramStories)
            }
            h if (h.contains("instagram.com") || h.contains("instagr.am"))
                && instagram_is_profile_path(url) =>
            {
                Ok(Self::InstagramProfile)
            }
            h if h.contains("instagram.com") || h.contains("instagr.am") => {
                Ok(Self::InstagramDownload)
            }
            h if h.contains("facebook.com") || h.contains("fb.watch") || h.contains("fb.com") => {
                Ok(Self::FacebookDownload)
            }
            h if h.contains("threads.net") => Ok(Self::ThreadsDownload),
            h if h.contains("reddit.com") || h.contains("redd.it") => Ok(Self::RedditDownload),
            h if h.contains("pinterest.com") || h.contains("pin.it") => Ok(Self::PinterestDownload),
            h if h.contains("terabox.com")
                || h.contains("nephobox.com")
                || h.contains("dubox.com")
                || h.contains("teraboxapp.com")
                || h.contains("1024terabox.com")
                || h.contains("terabox.app")
                || h.contains("terabox.link") =>
            {
                Ok(Self::TeraboxDownload)
            }
            h if h.contains("spotify.com") => Ok(Self::SpotifyDownload),
            h if h.contains("soundcloud.com") => Ok(Self::SoundcloudDownload),
            h if h.contains("capcut.com") => Ok(Self::CapcutDownload),
            h if h.contains("linkedin.com") => Ok(Self::LinkedinDownload),
            _ => anyhow::bail!("unsupported domain: {host}"),
        }
    }
}

fn instagram_is_profile_path(url: &url::Url) -> bool {
    let Some(mut segments) = url.path_segments() else {
        return false;
    };
    let Some(segment) = segments.next() else {
        return false;
    };
    if !segment.is_empty()
        && segment != "p"
        && segment != "reel"
        && segment != "reels"
        && segment != "tv"
        && segment != "stories"
    {
        return segments.next().is_none();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
