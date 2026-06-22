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
            .ok_or_else(|| anyhow::anyhow!("no host in URL"))?;
        let path = url.path();

        // Case-insensitive matching without allocating (needles are ASCII-lowercase):
        // `contains_ci` / `ends_with_ci` replace the old `host.to_lowercase()` allocations.
        match host {
            h if contains_ci(h, "youtube.com")
                || contains_ci(h, "youtu.be")
                || contains_ci(h, "yewtu.be")
                || contains_ci(h, "inv.nadeko.net") =>
            {
                Ok(Self::YoutubeDownload)
            }
            h if contains_ci(h, "tiktok.com") && contains_ci(path, "/music/") => {
                Ok(Self::TiktokMusic)
            }
            h if contains_ci(h, "tiktok.com")
                && contains_ci(path, "/@")
                && !contains_ci(path, "/video/")
                && !contains_ci(path, "/photo/") =>
            {
                Ok(Self::TiktokProfile)
            }
            h if contains_ci(h, "tiktok.com") => Ok(Self::TiktokDownload),
            h if h.eq_ignore_ascii_case("twitter.com")
                || ends_with_ci(h, ".twitter.com")
                || h.eq_ignore_ascii_case("x.com")
                || ends_with_ci(h, ".x.com")
                || h.eq_ignore_ascii_case("t.co")
                || ends_with_ci(h, ".t.co") =>
            {
                Ok(Self::TwitterDownload)
            }
            h if (contains_ci(h, "instagram.com") || contains_ci(h, "instagr.am"))
                && (contains_ci(path, "/stories/") || contains_ci(path, "/story/")) =>
            {
                Ok(Self::InstagramStories)
            }
            h if (contains_ci(h, "instagram.com") || contains_ci(h, "instagr.am"))
                && instagram_is_profile_path(url) =>
            {
                Ok(Self::InstagramProfile)
            }
            h if contains_ci(h, "instagram.com") || contains_ci(h, "instagr.am") => {
                Ok(Self::InstagramDownload)
            }
            h if contains_ci(h, "facebook.com")
                || contains_ci(h, "fb.watch")
                || contains_ci(h, "fb.com") =>
            {
                Ok(Self::FacebookDownload)
            }
            h if contains_ci(h, "threads.net") => Ok(Self::ThreadsDownload),
            h if contains_ci(h, "reddit.com") || contains_ci(h, "redd.it") => {
                Ok(Self::RedditDownload)
            }
            h if contains_ci(h, "pinterest.com") || contains_ci(h, "pin.it") => {
                Ok(Self::PinterestDownload)
            }
            h if contains_ci(h, "terabox.com")
                || contains_ci(h, "nephobox.com")
                || contains_ci(h, "dubox.com")
                || contains_ci(h, "teraboxapp.com")
                || contains_ci(h, "1024terabox.com")
                || contains_ci(h, "terabox.app")
                || contains_ci(h, "terabox.link") =>
            {
                Ok(Self::TeraboxDownload)
            }
            h if contains_ci(h, "spotify.com") => Ok(Self::SpotifyDownload),
            h if contains_ci(h, "soundcloud.com") => Ok(Self::SoundcloudDownload),
            h if contains_ci(h, "capcut.com") => Ok(Self::CapcutDownload),
            h if contains_ci(h, "linkedin.com") => Ok(Self::LinkedinDownload),
            _ => anyhow::bail!("unsupported domain: {host}"),
        }
    }
}

/// ASCII case-insensitive substring check, allocation-free. `needle` must be lowercase.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() {
        return true;
    }
    if nb.len() > hb.len() {
        return false;
    }
    hb.windows(nb.len())
        .any(|w| w.iter().zip(nb).all(|(c, n)| c.eq_ignore_ascii_case(n)))
}

/// ASCII case-insensitive suffix check, allocation-free. `suffix` must be lowercase.
fn ends_with_ci(haystack: &str, suffix: &str) -> bool {
    let (hb, sb) = (haystack.as_bytes(), suffix.as_bytes());
    hb.len() >= sb.len()
        && hb[hb.len() - sb.len()..]
            .iter()
            .zip(sb)
            .all(|(c, n)| c.eq_ignore_ascii_case(n))
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
