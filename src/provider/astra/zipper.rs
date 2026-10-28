use std::io::Write;
use futures::stream::{self, StreamExt};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::provider::{MediaItem, MediaKind, MediaMeta, MediaReader};
use super::client::sanitize_filename;
use super::concurrency::dynamic_download_concurrency;
use super::stream::image_ext;
use super::types::AstraDownloadItem;

pub async fn build_in_ram_zip(
    client: reqwest::Client,
    slideshow_items: Vec<AstraDownloadItem>,
    title: Option<String>,
    description: Option<String>,
) -> anyhow::Result<MediaItem> {
    let total_pages = slideshow_items.len();
    // Pre-allocate buffer in RAM: ~2MB per page (bounded up to 250MB for safety on 1GB RAM)
    let estimated_bytes = (total_pages * 2 * 1024 * 1024).min(250 * 1024 * 1024);
    let zip_buf = std::io::Cursor::new(Vec::with_capacity(estimated_bytes));
    let mut zip_writer = zip::ZipWriter::new(zip_buf);
    let zip_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored); // 0% CPU, pure memory copy!

    let concurrency = dynamic_download_concurrency();

    let mut fetch_stream = stream::iter(slideshow_items.into_iter().enumerate())
        .map(|(idx, item)| {
            let client = client.clone();
            async move {
                let mut attempts = 0;
                loop {
                    attempts += 1;
                    let resp = client.get(&item.url).send().await;
                    match resp {
                        Ok(r) if r.status().is_success() => {
                            let ctype = r
                                .headers()
                                .get("content-type")
                                .and_then(|v| v.to_str().ok())
                                .map(String::from);
                            if let Ok(bytes) = r.bytes().await {
                                return (idx, Some(bytes), ctype);
                            }
                        }
                        _ => {}
                    }
                    if attempts >= 3 {
                        tracing::warn!(page = idx, "failed to download page after 3 attempts");
                        return (idx, None, None);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempts)).await;
                }
            }
        })
        .buffered(concurrency);

    while let Some((idx, bytes_opt, ctype)) = fetch_stream.next().await {
        if let Some(bytes) = bytes_opt {
            let ext = image_ext(ctype.as_deref()).unwrap_or("jpg");
            let entry_name = format!("{:03}_p{}.{}", idx + 1, idx, ext);
            if let Err(e) = zip_writer.start_file(&entry_name, zip_options) {
                tracing::warn!("failed to start zip entry {}: {}", entry_name, e);
                continue;
            }
            let _ = zip_writer.write_all(&bytes);
        }
    }

    let mut cursor = zip_writer.finish()?;
    let total_size = cursor.get_ref().len() as u64;
    cursor.set_position(0);
    let sanitized = sanitize_filename(title.as_deref().unwrap_or("gallery"), "gallery");
    let filename = format!("{sanitized}.zip");

    let meta = MediaMeta {
        filename,
        mime_type: "application/zip".into(),
        size: total_size,
        duration_secs: None,
        dims: None,
        kind: MediaKind::Photo,
        title,
        description,
        thumbnail_url: None,
    };

    let reader: MediaReader = Box::pin(cursor);
    Ok(MediaItem { meta, reader })
}
