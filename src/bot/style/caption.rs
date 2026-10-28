use std::time::Duration;

use super::format::{format_bytes, format_duration, format_resolution};
use super::html::html_escape;

#[derive(Clone, Default)]
pub struct CaptionOptions<'a> {
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub source_url: &'a str,
    pub type_str: &'a str,
    pub resolution: Option<(i32, i32)>,
    pub total_bytes: u64,
    pub duration: Option<Duration>,
    pub elapsed: Duration,
    pub cache_label: Option<&'a str>,
    pub media_hash: Option<&'a str>,
    pub sender_name: Option<&'a str>,
    pub bot_username: Option<&'a str>,
}

pub fn detect_platform(url: &str) -> Option<&'static str> {
    let lower = url.to_lowercase();
    if lower.contains("instagram.com") || lower.contains("instagr.am") {
        Some("Instagram")
    } else if lower.contains("tiktok.com") {
        Some("TikTok")
    } else if lower.contains("twitter.com") || lower.contains("x.com") {
        Some("X")
    } else if lower.contains("facebook.com") || lower.contains("fb.watch") {
        Some("Facebook")
    } else if lower.contains("threads.net") {
        Some("Threads")
    } else if lower.contains("reddit.com") || lower.contains("redd.it") {
        Some("Reddit")
    } else if lower.contains("pinterest.com") || lower.contains("pin.it") {
        Some("Pinterest")
    } else if lower.contains("pixiv.net") {
        Some("Pixiv")
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        Some("YouTube")
    } else if lower.contains("spotify.com") {
        Some("Spotify")
    } else if lower.contains("soundcloud.com") {
        Some("SoundCloud")
    } else if lower.contains("capcut.com") {
        Some("CapCut")
    } else if lower.contains("terabox.com") || lower.contains("1024tera.com") {
        Some("TeraBox")
    } else {
        None
    }
}

pub fn get_source_link(url: &str) -> String {
    let escaped_url = html_escape(url);
    if let Some(platform) = detect_platform(url) {
        return format!("<a href=\"{escaped_url}\">{platform}</a>");
    }
    if let Ok(parsed) = url::Url::parse(url)
        && let Some(domain) = parsed.domain()
    {
        let escaped_domain = html_escape(domain);
        return format!("<a href=\"{escaped_url}\">{escaped_domain}</a>");
    }
    format!("<a href=\"{escaped_url}\">Link</a>")
}

pub fn build_media_caption(opts: CaptionOptions<'_>) -> String {
    let mut quote_content = String::new();
    if let Some(t) = opts.title {
        let escaped_t = html_escape(t);
        quote_content.push_str(&format!("<b>{escaped_t}</b>\n"));
    }
    if let Some(desc) = opts.description {
        let escaped_desc = html_escape(desc);
        quote_content.push_str(&escaped_desc);
    }
    let quote_content = quote_content.trim();

    let mut metadata = String::new();
    metadata.push_str(&format!("📎 <b>Source:</b> {}\n", get_source_link(opts.source_url)));
    metadata.push_str(&format!("📄 <b>Type:</b> {}\n", opts.type_str));

    if let Some(dims) = opts.resolution {
        metadata.push_str(&format!("📺 <b>Resolution:</b> {}\n", format_resolution(dims)));
    }
    if let Some(dur) = opts.duration {
        metadata.push_str(&format!("⏱ <b>Duration:</b> {}\n", format_duration(dur)));
    }
    if opts.total_bytes > 0 {
        metadata.push_str(&format!("📁 <b>Size:</b> {}\n", format_bytes(opts.total_bytes)));
    }

    let elapsed_str = format_duration(opts.elapsed);
    if let Some(label) = opts.cache_label {
        metadata.push_str(&format!("⏱ <b>Process Time:</b> {elapsed_str} (<code>{label}</code>)\n"));
    } else {
        metadata.push_str(&format!("⏱ <b>Process Time:</b> {elapsed_str}\n"));
    }

    if let Some(hash) = opts.media_hash {
        let short_hash = if hash.len() >= 16 { &hash[..16] } else { hash };
        metadata.push_str(&format!("🔑 <b>Hash:</b> <code>{short_hash}</code>\n"));
    }

    if let Some(name) = opts.sender_name {
        metadata.push_str(&format!("👤 <b>By:</b> {name}\n"));
    }
    if let Some(bot) = opts.bot_username.filter(|b| !b.is_empty()) {
        let bot = bot.trim_start_matches('@');
        metadata.push_str(&format!("🤖 Downloaded via @{bot}\n"));
    }

    metadata.push_str("\n💫 Powered by <a href=\"https://github.com/pavelc4/astra.git\">Astra</a>");

    if quote_content.is_empty() {
        metadata
    } else {
        format!("<blockquote>{quote_content}</blockquote>\n\n{metadata}")
    }
}
