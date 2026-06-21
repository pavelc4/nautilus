use std::sync::Arc;
use std::time::Duration;
use serde::Deserialize;
use crate::app::AppState;
use grammers_client::message::InputMessage;

#[derive(Deserialize)]
struct AstraHealthResponse {
    data: Option<AstraHealthData>,
}

#[derive(Deserialize)]
struct AstraHealthData {
    version: String,
    uptime: String,
    cookies: AstraHealthCookies,
}

#[derive(Deserialize)]
struct AstraHealthCookies {
    instagram: bool,
    facebook: bool,
}

pub async fn cmd_check(state: &Arc<AppState>) -> anyhow::Result<InputMessage> {
    let api_url = state
        .config
        .astra_api_url
        .as_deref()
        .unwrap_or("http://localhost:3000");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let response = client
        .get(format!("{}/health", api_url))
        .send()
        .await;

    let (api_online, ig_loaded, fb_loaded, uptime, go_ver) = match response {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<AstraHealthResponse>().await {
                Ok(payload) => match payload.data {
                    Some(data) => (
                        true,
                        data.cookies.instagram,
                        data.cookies.facebook,
                        data.uptime,
                        data.version,
                    ),
                    None => (true, false, false, "unknown".to_string(), "unknown".to_string()),
                },
                Err(_) => (true, false, false, "unknown".to_string(), "unknown".to_string()),
            }
        }
        _ => (false, false, false, "offline".to_string(), "offline".to_string()),
    };

    let api_status = match (api_online, &go_ver[..]) {
        (true, ver) => format!("Online (Go {ver})"),
        (false, _) => "Offline (Connection error)".to_string(),
    };

    let ig_status = match (api_online, ig_loaded) {
        (false, _) => "Offline",
        (true, true) => "Loaded",
        (true, false) => "Missing / Expired",
    };

    let fb_status = match (api_online, fb_loaded) {
        (false, _) => "Offline",
        (true, true) => "Loaded",
        (true, false) => "Missing / Expired",
    };

    let platform_status = match api_online {
        true => "Operational",
        false => "Offline",
    };

    let text = format!(
        "Nautilus Diagnostics & Platform Status\n\n\
         Astra Backend API:\n\
         ├ Status: {}\n\
         ├ Uptime: {}\n\
         └ Cookies State:\n\
         │  ├ Instagram: {}\n\
         │  └ Facebook: {}\n\n\
         Platforms Operational Status:\n\
         ├ TikTok: {}\n\
         ├ Instagram: {}\n\
         ├ Facebook: {}\n\
         ├ YouTube: {}\n\
         ├ Twitter/X: {}\n\
         ├ Threads: {}\n\
         ├ SoundCloud: {}\n\
         ├ Spotify: {}\n\
         ├ Pinterest: {}\n\
         ├ LinkedIn: {}\n\
         └ TeraBox: {}",
        api_status,
        uptime,
        ig_status,
        fb_status,
        platform_status,
        if ig_loaded { "Operational" } else { "Limited (No Cookies)" },
        if fb_loaded { "Operational" } else { "Limited (No Cookies)" },
        platform_status,
        platform_status,
        platform_status,
        platform_status,
        platform_status,
        platform_status,
        platform_status,
        platform_status,
    );

    Ok(InputMessage::new().text(text))
}
