use std::io;
use std::time::Duration;

use reqwest::Client;
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;

use crate::provider::MediaReader;

pub fn http_client() -> Client {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(15))
        .build()
        .expect("reqwest client")
}

pub fn no_timeout_client() -> Client {
    Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .expect("reqwest client")
}

pub fn stream_to_reader(resp: reqwest::Response) -> MediaReader {
    let stream = resp
        .bytes_stream()
        .map(|r| r.map_err(io::Error::other));
    Box::pin(StreamReader::new(stream))
}

pub fn resolve_url(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("//") {
        format!("https:{url}")
    } else if url.starts_with('/') {
        let base = base.trim_end_matches('/');
        format!("{base}{url}")
    } else if url.contains('.') && !url.contains(' ') {
        format!("https://{url}")
    } else {
        url.to_string()
    }
}

pub async fn fetch_stream(
    client: &Client,
    url: &str,
) -> reqwest::Result<(u64, MediaReader)> {
    let resp = client.get(url).send().await?;
    let size = resp.content_length().unwrap_or(0);
    let reader = stream_to_reader(resp);
    Ok((size, reader))
}
