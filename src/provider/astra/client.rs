use crate::provider::{MediaItem, MediaKind, MediaMeta, MediaReader};
use tokio_stream::StreamExt;

pub fn parse_quality(q: &str) -> u32 {
    let clean: String = q.chars().filter(|c| c.is_ascii_digit()).collect();
    clean.parse().unwrap_or(0)
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

pub async fn fetch_stream(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<(u64, MediaReader)> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Failed to fetch stream from CDN: {}", resp.status());
    }
    let size = resp.content_length().unwrap_or(0);
    let stream = resp
        .bytes_stream()
        .map(|r| r.map_err(std::io::Error::other));
    let stream_reader = tokio_util::io::StreamReader::new(stream);
    let buffered = tokio::io::BufReader::with_capacity(128 * 1024, stream_reader);
    let reader: MediaReader = Box::pin(buffered);
    Ok((size, reader))
}

pub async fn download_item(
    client: &reqwest::Client,
    url: String,
    kind: MediaKind,
    mime_type: std::borrow::Cow<'static, str>,
    filename: String,
    title: Option<String>,
    description: Option<String>,
) -> anyhow::Result<MediaItem> {
    let (size, reader) = fetch_stream(client, &url).await?;
    let meta = MediaMeta {
        filename,
        mime_type,
        size,
        duration_secs: None,
        dims: None,
        kind,
        title,
        description,
    };
    Ok(MediaItem { meta, reader })
}
