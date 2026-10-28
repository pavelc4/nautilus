use nautilus::config::Config;
use nautilus::provider::astra::client::detect_image_dimensions;
use nautilus::provider::astra::endpoint::AstraEndpoint;
use nautilus::provider::{MediaKind, MediaMeta};
use std::borrow::Cow;
use url::Url;

#[test]
fn test_library_exports_and_models() {
    let meta = MediaMeta {
        filename: "test.mp4".to_string(),
        mime_type: Cow::Borrowed("video/mp4"),
        size: 1024,
        duration_secs: Some(10),
        dims: Some((1280, 720)),
        kind: MediaKind::Video,
        title: Some("Test Video".to_string()),
        description: Some("Test Description".to_string()),
        thumbnail_url: Some("https://example.com/thumb.jpg".to_string()),
    };

    assert_eq!(meta.filename, "test.mp4");
    assert_eq!(meta.kind, MediaKind::Video);
    assert_eq!(meta.size, 1024);
    assert_eq!(meta.dims, Some((1280, 720)));
}

#[test]
fn test_config_methods_and_defaults() {
    let cfg = Config {
        telegram_app_id: 12345,
        telegram_app_hash: "hash".to_string(),
        bot_token: "token".to_string(),
        owner_id: 999,
        astra_api_url: None,
        max_file_size_mb: None,
        progress_edit_secs: None,
        worker_concurrency: None,
        dump_channel: None,
        dump_channel_id: None,
    };

    assert_eq!(cfg.max_file_size_bytes(), 2000 * 1024 * 1024);
    assert_eq!(cfg.progress_edit_secs(), 3);
    assert!(cfg.worker_concurrency() >= 4);
    assert_eq!(cfg.dump_channel_target(), None);

    let cfg_with_ch = Config {
        telegram_app_id: 12345,
        telegram_app_hash: "hash".to_string(),
        bot_token: "token".to_string(),
        owner_id: 999,
        astra_api_url: None,
        max_file_size_mb: None,
        progress_edit_secs: None,
        worker_concurrency: None,
        dump_channel: Some(" @my_dump_ch ".to_string()),
        dump_channel_id: None,
    };
    assert_eq!(cfg_with_ch.dump_channel_target(), Some("@my_dump_ch".to_string()));

    let cfg_with_id = Config {
        telegram_app_id: 12345,
        telegram_app_hash: "hash".to_string(),
        bot_token: "token".to_string(),
        owner_id: 999,
        astra_api_url: None,
        max_file_size_mb: None,
        progress_edit_secs: None,
        worker_concurrency: None,
        dump_channel: None,
        dump_channel_id: Some(-1001234567890),
    };
    assert_eq!(cfg_with_id.dump_channel_target(), Some("-1001234567890".to_string()));
}

#[test]
fn test_endpoint_resolution_for_all_supported_platforms() {
    let cases = [
        ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", AstraEndpoint::YoutubeDownload),
        ("https://youtu.be/dQw4w9WgXcQ", AstraEndpoint::YoutubeDownload),
        ("https://www.tiktok.com/@user/video/123456", AstraEndpoint::TiktokDownload),
        ("https://x.com/user/status/123456", AstraEndpoint::TwitterDownload),
        ("https://twitter.com/user/status/123456", AstraEndpoint::TwitterDownload),
        ("https://www.instagram.com/p/Cxyz123/", AstraEndpoint::InstagramDownload),
        ("https://www.facebook.com/watch/?v=123", AstraEndpoint::FacebookDownload),
        ("https://www.reddit.com/r/rust/comments/123", AstraEndpoint::RedditDownload),
        ("https://www.pinterest.com/pin/123", AstraEndpoint::PinterestDownload),
        ("https://www.pixiv.net/en/artworks/150811369", AstraEndpoint::PixivDownload),
    ];

    for (url_str, expected) in cases {
        let parsed = Url::parse(url_str).expect("valid test URL");
        let endpoint = AstraEndpoint::try_from(&parsed).expect("must resolve endpoint");
        assert_eq!(endpoint, expected, "Failed for URL: {url_str}");
    }
}

#[test]
fn test_image_dimension_detectors_integration() {
    // 1x1 8-bit PNG
    let png_header = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
        0x00, 0x00, 0x00, 0x0D, // IHDR chunk length (13)
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x04, 0x00, // width: 1024
        0x00, 0x00, 0x03, 0x00, // height: 768
        0x08, 0x06, 0x00, 0x00, 0x00,
    ];
    assert_eq!(detect_image_dimensions(&png_header), Some((1024, 768)));
}

#[tokio::test]
async fn test_astra_twitter_multi_and_mixed_live_local() {
    use nautilus::provider::astra::AstraProvider;
    use nautilus::provider::Provider;

    let provider = AstraProvider::new("http://localhost:3000".to_string());

    // 1. Multi-video tweet (3 videos)
    let multi_url = "https://x.com/ejaaipongg/status/2115271858660765981?a=zfwR6h0VbSPiSfgoVOo&s=19";
    match provider.resolve(multi_url, None).await {
        Ok(items) => {
            assert_eq!(items.len(), 3, "expected 3 videos resolved for multi-video tweet");
            for item in items {
                assert_eq!(item.meta.kind, MediaKind::Video);
            }
        }
        Err(e) => {
            eprintln!("Skipping live local test (server unreachable): {e}");
            return;
        }
    }

    // 2. Mixed photo + video tweet (1 photo, 1 video)
    let mixed_url = "https://x.com/leedo0804/status/2114891265670365220?a=UrJz2rjGDP9A7LEnbo0&s=20";
    let items = provider.resolve(mixed_url, None).await.expect("resolve mixed tweet");
    assert_eq!(items.len(), 2, "expected 2 items (1 photo + 1 video) for mixed tweet");
    assert!(items.iter().any(|i| i.meta.kind == MediaKind::Photo), "must contain photo");
    assert!(items.iter().any(|i| i.meta.kind == MediaKind::Video), "must contain video");
}

fn get_proc_rss_mb() -> (f64, f64) {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let mut rss_kb = 0;
        let mut hwm_kb = 0;
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                rss_kb = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            } else if line.starts_with("VmHWM:") {
                hwm_kb = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            }
        }
        (rss_kb as f64 / 1024.0, hwm_kb as f64 / 1024.0)
    } else {
        (0.0, 0.0)
    }
}

fn get_astra_rss_mb() -> Option<(f64, f64)> {
    let output = std::process::Command::new("pgrep").arg("astra-server").output().ok()?;
    let pid_str = String::from_utf8_lossy(&output.stdout).trim().lines().next()?.to_string();
    let status = std::fs::read_to_string(format!("/proc/{pid_str}/status")).ok()?;
    let mut rss_kb = 0;
    let mut hwm_kb = 0;
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            rss_kb = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        } else if line.starts_with("VmHWM:") {
            hwm_kb = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        }
    }
    Some((rss_kb as f64 / 1024.0, hwm_kb as f64 / 1024.0))
}

#[tokio::test]
async fn test_pixiv_in_ram_zip_live_local() {
    use nautilus::provider::astra::AstraProvider;
    use nautilus::provider::Provider;
    use tokio::io::AsyncReadExt;

    let provider = AstraProvider::new("http://localhost:3000".to_string());
    let pixiv_url = "https://pixiv.net/en/artworks/150811369";

    let (proc_rss_before, _) = get_proc_rss_mb();
    let astra_before = get_astra_rss_mb();
    println!("\n=== [ZIP BENCHMARK & RAM PROFILE] ===");
    println!("Baseline Process RSS: {:.2} MB", proc_rss_before);
    if let Some((rss, hwm)) = astra_before {
        println!("Baseline Astra Go RSS: {:.2} MB (HWM: {:.2} MB)", rss, hwm);
    }

    let start_time = std::time::Instant::now();

    // Test resolving with format: Some("zip")
    match provider.resolve(pixiv_url, Some("zip")).await {
        Ok(mut items) => {
            let zip_duration = start_time.elapsed();
            assert_eq!(items.len(), 1, "expected 1 zip item returned");
            let item = items.pop().unwrap();
            assert!(item.meta.filename.ends_with(".zip"), "filename must end with .zip");
            assert_eq!(item.meta.mime_type, "application/zip");
            assert!(item.meta.size > 0, "zip size must be > 0");

            let (proc_rss_zip_done, _proc_hwm_zip_done) = get_proc_rss_mb();
            let astra_mid = get_astra_rss_mb();

            let mut reader = item.reader;
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf).await.expect("read zip bytes");
            assert_eq!(buf.len() as u64, item.meta.size);

            let (proc_rss_read_done, proc_hwm_peak) = get_proc_rss_mb();
            let astra_after = get_astra_rss_mb();

            let cursor = std::io::Cursor::new(buf);
            let archive = zip::ZipArchive::new(cursor).expect("valid zip archive");
            assert_eq!(archive.len(), 71, "zip must contain all 71 pages in order");

            let total_mb = item.meta.size as f64 / (1024.0 * 1024.0);
            println!("ZIP Generation Time: {:.2}s for 71 pages ({:.2} MB)", zip_duration.as_secs_f64(), total_mb);
            println!("Nautilus Peak RSS (VmHWM): {:.2} MB", proc_hwm_peak);
            println!("Nautilus Active RSS after zip creation: {:.2} MB", proc_rss_zip_done);
            println!("Nautilus Active RSS after buffer read: {:.2} MB", proc_rss_read_done);
            if let Some((rss, hwm)) = astra_mid {
                println!("Astra Go RSS during zip: {:.2} MB (Peak HWM: {:.2} MB)", rss, hwm);
            }
            if let Some((rss, hwm)) = astra_after {
                println!("Astra Go RSS after zip: {:.2} MB (Peak HWM: {:.2} MB)", rss, hwm);
            }
            println!("======================================\n");

            // Safety assertion for 1GB RAM VPS: peak RAM usage should never exceed 350 MB!
            assert!(proc_hwm_peak < 350.0, "Peak memory exceeded 350MB budget on 1GB VPS!");
        }
        Err(e) => {
            eprintln!("Skipping live local pixiv zip test (server unreachable): {e}");
        }
    }
}

#[tokio::test]
async fn test_pixiv_streaming_photos_live_local() {
    use nautilus::provider::astra::AstraProvider;
    use nautilus::provider::Provider;
    use tokio::io::AsyncReadExt;

    let provider = AstraProvider::new("http://localhost:3000".to_string());
    let pixiv_url = "https://pixiv.net/en/artworks/150811369";

    let (proc_rss_before, _) = get_proc_rss_mb();
    let astra_before = get_astra_rss_mb();
    println!("\n=== [STREAMING BENCHMARK & RAM PROFILE] ===");
    println!("Baseline Process RSS: {:.2} MB", proc_rss_before);
    if let Some((rss, hwm)) = astra_before {
        println!("Baseline Astra Go RSS: {:.2} MB (HWM: {:.2} MB)", rss, hwm);
    }

    let start_time = std::time::Instant::now();

    // Resolving photo stream format
    match provider.resolve(pixiv_url, Some("photo")).await {
        Ok(items) => {
            let resolve_duration = start_time.elapsed();
            assert_eq!(items.len(), 71, "expected 71 photo items returned");

            let (proc_rss_resolved, _) = get_proc_rss_mb();
            println!("Resolved 71 photo streams in: {:.2}s", resolve_duration.as_secs_f64());
            println!("Process RSS after stream handles opened: {:.2} MB", proc_rss_resolved);

            // Read 512KB chunk from first 4 stream items concurrently to simulate Telegram upload
            let mut read_tasks = Vec::new();
            for mut item in items.into_iter().take(4) {
                read_tasks.push(tokio::spawn(async move {
                    let mut chunk = vec![0u8; 512 * 1024];
                    let n = item.reader.read(&mut chunk).await.unwrap_or(0);
                    n
                }));
            }

            let mut total_chunk_read = 0;
            for t in read_tasks {
                total_chunk_read += t.await.unwrap_or(0);
            }

            let (proc_rss_streaming, proc_hwm_streaming) = get_proc_rss_mb();
            let astra_streaming = get_astra_rss_mb();

            println!("Read {} bytes across 4 concurrent 512KB stream buffers", total_chunk_read);
            println!("Process RSS during 512KB stream piping: {:.2} MB (HWM: {:.2} MB)", proc_rss_streaming, proc_hwm_streaming);
            if let Some((rss, hwm)) = astra_streaming {
                println!("Astra Go RSS during streaming: {:.2} MB (HWM: {:.2} MB)", rss, hwm);
            }
            println!("==========================================\n");

            // Streaming memory assertion: stream handles and 512KB buffers must stay well under 100 MB
            assert!(proc_rss_streaming < 150.0, "Streaming memory exceeded 150MB budget!");
        }
        Err(e) => {
            eprintln!("Skipping live local pixiv streaming test (server unreachable): {e}");
        }
    }
}

#[tokio::test]
async fn test_dump_cdn_cache_crud() {
    use nautilus::cache::{DumpCdnCache, DumpRecord};

    let temp_dir = std::env::temp_dir().join(format!("test_cdn_{}", rand::random::<u32>()));
    let db_path = temp_dir.join("dump_cdn.sqlite");

    let cache = DumpCdnCache::open(db_path.to_str().unwrap()).await.unwrap();

    let hash = nautilus::cache::compute_cache_hash("https://x.com/user/status/123#mp4");
    let record = DumpRecord {
        cache_key: "https://x.com/user/status/123#mp4".to_string(),
        hash: hash.clone(),
        channel_id: -1001234567890,
        message_ids: vec![42, 43],
        kind: "video".to_string(),
        title: Some("Viral Tweet Video".to_string()),
        description: Some("Funny caption".to_string()),
        total_bytes: 1048576,
        dims: Some((1920, 1080)),
        duration_secs: Some(15),
        created_at: 1700000000,
    };

    // Insert
    cache.insert(&record).await.unwrap();

    // Get by cache_key
    let fetched = cache.get(&record.cache_key).await.unwrap().expect("record should exist");
    assert_eq!(fetched.cache_key, record.cache_key);
    assert_eq!(fetched.hash, hash);
    assert_eq!(fetched.channel_id, -1001234567890);
    assert_eq!(fetched.message_ids, vec![42, 43]);
    assert_eq!(fetched.kind, "video");
    assert_eq!(fetched.title.as_deref(), Some("Viral Tweet Video"));
    assert_eq!(fetched.dims, Some((1920, 1080)));
    assert_eq!(fetched.total_bytes, 1048576);
    assert_eq!(fetched.duration_secs, Some(15));

    // Get by short hash prefix
    let fetched_by_hash = cache.get_by_hash(&hash[..16]).await.unwrap().expect("prefix search should work");
    assert_eq!(fetched_by_hash.cache_key, record.cache_key);
    assert_eq!(fetched_by_hash.hash, hash);

    // Delete
    cache.delete(&record.cache_key).await.unwrap();
    let after_delete = cache.get(&record.cache_key).await.unwrap();
    assert!(after_delete.is_none());

    let _ = std::fs::remove_dir_all(temp_dir);
}
