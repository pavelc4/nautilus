pub use super::stream::{detect_image_dimensions, parse_dimensions};

pub fn parse_quality(quality: &str, label: &str) -> u32 {
    let score = |s: &str| {
        let clean: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty()
            && let Ok(num) = clean.parse::<u32>()
        {
            return Some(num);
        }
        let lower = s.to_lowercase();
        match () {
            _ if lower.contains("hd") || lower.contains("high") || lower.contains("original") => {
                Some(1080)
            }
            _ if lower.contains("medium") => Some(720),
            _ if lower.contains("sd") || lower.contains("low") => Some(360),
            _ => None,
        }
    };

    score(quality).or_else(|| score(label)).unwrap_or(0)
}

pub fn sanitize_filename(title: &str, default: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim_matches('_');
    if clean.is_empty() {
        default.to_string()
    } else {
        clean.to_string()
    }
}

pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(300)) // 5 minutes timeout for downloading large media
        .tcp_nodelay(true)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
