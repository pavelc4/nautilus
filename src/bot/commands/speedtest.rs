use std::time::{Duration, Instant};
use grammers_client::message::InputMessage;

pub async fn cmd_speedtest() -> anyhow::Result<InputMessage> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 1. Latency Check
    let start_ping = Instant::now();
    let ping_resp = client.get("https://speed.cloudflare.com/").send().await;
    let latency_ms = match ping_resp {
        Ok(_) => Some(start_ping.elapsed().as_micros() as f64 / 1000.0),
        Err(_) => None,
    };

    // 2. Download Speed (10MB test, max 3 seconds timeout)
    let down_resp = client
        .get("https://speed.cloudflare.com/__down?bytes=10485760")
        .send()
        .await;

    let mut bytes_downloaded = 0;
    let mut down_duration = Duration::from_secs(1);

    if let Ok(mut resp) = down_resp {
        if resp.status().is_success() {
            let stream_start = Instant::now();
            while let Ok(Some(chunk)) = resp.chunk().await {
                bytes_downloaded += chunk.len();
                if stream_start.elapsed() >= Duration::from_secs(3) {
                    break;
                }
            }
            down_duration = stream_start.elapsed();
        }
    }

    let down_speed_mbps = if bytes_downloaded > 0 && down_duration.as_secs_f64() > 0.0 {
        let sec = down_duration.as_secs_f64();
        let bits = (bytes_downloaded as f64) * 8.0;
        Some(bits / sec / 1_000_000.0)
    } else {
        None
    };

    let down_speed_mbs = down_speed_mbps.map(|mbps| mbps / 8.0);

    // 3. Upload Speed (2MB test, max 3 seconds timeout)
    let upload_data = vec![0u8; 2 * 1024 * 1024]; // 2MB
    let start_up = Instant::now();
    let up_resp = client
        .post("https://speed.cloudflare.com/__up")
        .body(upload_data)
        .send()
        .await;

    let up_duration = start_up.elapsed();

    let (up_speed_mbps, up_speed_mbs) = match up_resp {
        Ok(resp) if resp.status().is_success() => {
            let sec = up_duration.as_secs_f64();
            if sec > 0.0 {
                let mbps = (2.0 * 8.0) / sec;
                (Some(mbps), Some(2.0 / sec))
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    };

    // Format results cleanly, without excessive emojis
    let latency_str = match latency_ms {
        Some(ms) => format!("{ms:.2} ms"),
        None => "Error".to_string(),
    };

    let down_str = match (down_speed_mbps, down_speed_mbs) {
        (Some(mbps), Some(mbs)) => format!("{mbps:.2} Mbps ({mbs:.2} MB/s)"),
        _ => "Error".to_string(),
    };

    let up_str = match (up_speed_mbps, up_speed_mbs) {
        (Some(mbps), Some(mbs)) => format!("{mbps:.2} Mbps ({mbs:.2} MB/s)"),
        _ => "Error".to_string(),
    };

    let text = format!(
        "Nautilus Network Speedtest\n\n\
         Cloudflare Edge:\n\
         ├ Latency: {}\n\
         ├ Download: {}\n\
         └ Upload: {}",
        latency_str,
        down_str,
        up_str
    );

    Ok(InputMessage::new().text(text))
}
