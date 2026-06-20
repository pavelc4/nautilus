use async_trait::async_trait;
use reqwest;
use scraper::{Html, Selector};

use crate::provider::scraper::helpers;
use crate::provider::scraper::Extractor;
use crate::provider::{MediaItem, MediaKind, MediaMeta, MediaReader};

pub struct RedditExtractor;

/// Extract a human-readable title from the Reddit URL slug.
fn title_from_url(url: &str) -> Option<String> {
    // URL pattern: /r/Sub/comments/ID/slug_text/
    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    // Find the part after "comments" and the post ID (index+2)
    if let Some(pos) = parts.iter().position(|p| *p == "comments") {
        if let Some(slug) = parts.get(pos + 2) {
            let title = slug.replace('_', " ").replace('-', " ");
            let mut chars = title.chars();
            if let Some(first) = chars.next() {
                return Some(format!("{}{}", first.to_uppercase(), chars.as_str()));
            }
        }
    }
    None
}

/// Parse the rapidsave download link to extract video_url and audio_url parameters.
///
/// The `a.downloadbutton` href looks like:
/// `https://sd.rapidsave.com/download.php?permalink=...&video_url=<URL>&audio_url=<URL>`
fn parse_download_link(href: &str) -> Option<(String, Option<String>)> {
    let parsed = url::Url::parse(href).ok()?;
    let mut video_url = None;
    let mut audio_url = None;

    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "video_url" => video_url = Some(value.into_owned()),
            "audio_url" => audio_url = Some(value.into_owned()),
            _ => {}
        }
    }

    video_url.map(|v| (v, audio_url))
}

/// Intermediate struct to hold parsed data from rapidsave HTML.
/// Extracted synchronously so `Html` (non-Send) doesn't cross await points.
struct ParsedReddit {
    video: Option<(String, Option<String>)>, // (video_url, Option<audio_url>)
    images: Vec<String>,
}

/// Parse the rapidsave HTML and extract all media data synchronously.
fn parse_rapidsave_html(html: &str) -> ParsedReddit {
    let doc = Html::parse_document(html);

    // Try video: a.downloadbutton with download.php href
    let mut video = None;
    if let Ok(btn_sel) = Selector::parse("a.downloadbutton") {
        let download_href = doc
            .select(&btn_sel)
            .filter_map(|a| a.value().attr("href"))
            .find(|href| href.contains("download.php"));

        if let Some(href) = download_href {
            video = parse_download_link(href);
        }
    }

    // Fallback: image links (i.redd.it)
    let mut images = Vec::new();
    if let Ok(img_sel) = Selector::parse("a[href*='i.redd.it'], img[src*='i.redd.it']") {
        images = doc
            .select(&img_sel)
            .filter_map(|el| {
                el.value()
                    .attr("href")
                    .or_else(|| el.value().attr("src"))
                    .map(|s| helpers::resolve_url(s, "https://rapidsave.com"))
            })
            .collect();
    }

    ParsedReddit { video, images }
}

/// Merge video and audio streams using ffmpeg pipe (zero-disk, buffered to memory).
///
/// Spawns ffmpeg with two HTTP inputs, muxes to MP4, collects all output into
/// memory, and returns an exact `(size, AsyncRead)`. This ensures `upload_stream`
/// receives the correct byte count (ffmpeg muxed output ≠ video + audio sizes).
async fn merge_with_ffmpeg(
    video_url: &str,
    audio_url: &str,
) -> anyhow::Result<(u64, MediaReader)> {
    use tokio::process::Command;

    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            video_url,
            "-i",
            audio_url,
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-movflags",
            "frag_keyframe+empty_moov+faststart",
            "-f",
            "mp4",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg merge failed (exit {}): {stderr}", output.status.code().unwrap_or(-1));
    }

    let data = output.stdout;
    let size = data.len() as u64;
    if size == 0 {
        anyhow::bail!("ffmpeg produced empty output");
    }

    tracing::info!(merged_size = size, "ffmpeg merge complete");

    Ok((size, Box::pin(std::io::Cursor::new(data)) as MediaReader))
}

/// Extract the Reddit post ID from the URL for filename generation.
fn post_id_from_url(url: &str) -> &str {
    let parts: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(pos) = parts.iter().position(|p| *p == "comments") {
        if let Some(id) = parts.get(pos + 1) {
            return id;
        }
    }
    "unknown"
}

/// Extract `preview.redd.it` image filenames from old.reddit.com HTML (sync).
/// Returns filenames like `abc123.png` ready to be prefixed with `https://i.redd.it/`.
fn parse_old_reddit_images(html: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut filenames = Vec::new();

    // Match preview.redd.it/<filename>.<ext> — these are the gallery/post images.
    // We convert preview → i.redd.it for full-resolution originals.
    for cap in html.split("preview.redd.it/") {
        // Cap starts right after "preview.redd.it/"
        // Extract filename: alphanumeric + underscore, then dot, then extension
        let filename: String = cap
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-')
            .collect();

        // Must have an image extension
        if let Some(dot_pos) = filename.rfind('.') {
            let ext = &filename[dot_pos + 1..];
            if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp") && !filename.is_empty() {
                if seen.insert(filename.clone()) {
                    filenames.push(filename);
                }
            }
        }
    }

    filenames
}

/// Convert a Reddit URL to its old.reddit.com equivalent for scraping.
fn to_old_reddit_url(url: &str) -> String {
    url.replace("www.reddit.com", "old.reddit.com")
        .replace("//reddit.com", "//old.reddit.com")
}

#[async_trait]
impl Extractor for RedditExtractor {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("reddit.com/")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<Vec<MediaItem>> {
        let client = helpers::http_client();
        let post_id = post_id_from_url(url);
        let title = title_from_url(url);

        // === Step 1: Try rapidsave for video posts ===
        let info_url = format!("https://rapidsave.com/info?url={url}");
        let rapidsave_result = client.get(&info_url).send().await;
        let html_text = match rapidsave_result {
            Ok(resp) => resp.text().await?,
            Err(e) => {
                tracing::info!(url = %info_url, error = %e, "rapidsave failed, trying old.reddit.com fallback");
                // Proceed to old.reddit.com fallback
                let dl_client = helpers::no_timeout_client();
                let old_url = to_old_reddit_url(url);
                let old_resp = client.get(&old_url).send().await?;
                let old_html = old_resp.text().await?;

                let image_filenames = parse_old_reddit_images(&old_html);

                if !image_filenames.is_empty() {
                    let mut items = Vec::with_capacity(image_filenames.len());

                    for (i, filename) in image_filenames.iter().enumerate() {
                        let img_url = format!("https://i.redd.it/{filename}");
                        match helpers::fetch_stream(&dl_client, &img_url).await {
                            Ok((size, reader)) => {
                                let ext = filename
                                    .rsplit('.')
                                    .next()
                                    .unwrap_or("jpg");
                                let mime = match ext {
                                    "png" => "image/png",
                                    "gif" => "image/gif",
                                    "webp" => "image/webp",
                                    _ => "image/jpeg",
                                };
                                let out_name = format!("reddit_{post_id}_{}.{ext}", i + 1);
                                let meta = MediaMeta {
                                    filename: out_name,
                                    mime_type: mime.into(),
                                    size,
                                    duration_secs: None,
                                    dims: None,
                                    kind: MediaKind::Photo,
                                    title: if i == 0 { title.clone() } else { None },
                                    description: None,
                                };
                                items.push(MediaItem { meta, reader });
                            }
                            Err(e) => {
                                tracing::warn!(url = %img_url, error = %e, "skipping image");
                            }
                        }
                    }

                    if !items.is_empty() {
                        return Ok(items);
                    }
                }

                anyhow::bail!("no video or image found on Reddit post (old.reddit.com fallback)")
            }
        };

        // Parse HTML synchronously — scraper::Html is not Send,
        // so all DOM work must happen without crossing an await.
        let parsed = parse_rapidsave_html(&html_text);

        // --- Video path ---
        if let Some((video_url, audio_url)) = parsed.video {
            if let Some(ref audio) = audio_url {
                // Has audio — merge with ffmpeg (zero-disk, buffered to memory).
                let (size, reader) = merge_with_ffmpeg(&video_url, audio).await?;

                let filename = format!("reddit_{post_id}.mp4");
                let meta = MediaMeta {
                    filename,
                    mime_type: "video/mp4".into(),
                    size,
                    duration_secs: None,
                    dims: None,
                    kind: MediaKind::Video,
                    title,
                    description: None,
                };

                return Ok(vec![MediaItem { meta, reader }]);
            }

            // No audio — stream video directly.
            let (size, reader) = helpers::fetch_stream(&helpers::no_timeout_client(), &video_url).await?;

            let filename = format!("reddit_{post_id}.mp4");
            let meta = MediaMeta {
                filename,
                mime_type: "video/mp4".into(),
                size,
                duration_secs: None,
                dims: None,
                kind: MediaKind::Video,
                title,
                description: None,
            };

            return Ok(vec![MediaItem { meta, reader }]);
        }

        // --- Rapidsave image path (i.redd.it links in rapidsave page) ---
        if !parsed.images.is_empty() {
            let dl_client = helpers::no_timeout_client();
            let mut items = Vec::with_capacity(parsed.images.len());

            for (i, img_url) in parsed.images.iter().enumerate() {
                let (size, reader) = helpers::fetch_stream(&dl_client, img_url).await?;
                let ext = if img_url.contains(".png") {
                    "png"
                } else if img_url.contains(".gif") {
                    "gif"
                } else {
                    "jpg"
                };
                let mime = match ext {
                    "png" => "image/png",
                    "gif" => "image/gif",
                    _ => "image/jpeg",
                };
                let filename = format!("reddit_{post_id}_{}.{ext}", i + 1);
                let meta = MediaMeta {
                    filename,
                    mime_type: mime.into(),
                    size,
                    duration_secs: None,
                    dims: None,
                    kind: MediaKind::Photo,
                    title: if i == 0 { title.clone() } else { None },
                    description: None,
                };
                items.push(MediaItem { meta, reader });
            }

            return Ok(items);
        }

        anyhow::bail!("no video or image found on Reddit post")
    }
}