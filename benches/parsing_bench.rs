use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Duration;

use nautilus::bot::commands::dl::{format_process_duration, normalize_url};
use nautilus::cache::BoundedCache;
use nautilus::provider::astra::client::{detect_image_dimensions, parse_dimensions, parse_quality};

fn bench_url_normalization(c: &mut Criterion) {
    let raw_url = "https://x.com/user/status/1234567890?utm_source=twitter&utm_medium=social&igsh=abcdef&keep_param=1";

    c.bench_function("normalize_url_with_tracking", |b| {
        b.iter(|| {
            normalize_url(black_box(raw_url));
        });
    });
}

fn bench_duration_formatting(c: &mut Criterion) {
    let dur_ms = Duration::from_millis(345);
    let dur_secs = Duration::from_millis(14_560);
    let dur_mins = Duration::from_secs(145);

    c.bench_function("format_process_duration_subsecond", |b| {
        b.iter(|| {
            format_process_duration(black_box(dur_ms));
        });
    });

    c.bench_function("format_process_duration_seconds", |b| {
        b.iter(|| {
            format_process_duration(black_box(dur_secs));
        });
    });

    c.bench_function("format_process_duration_minutes", |b| {
        b.iter(|| {
            format_process_duration(black_box(dur_mins));
        });
    });
}

fn bench_image_dimension_detection(c: &mut Criterion) {
    // 1920x1080 PNG header
    let mut png_bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R']);
    png_bytes.extend_from_slice(&1920u32.to_be_bytes());
    png_bytes.extend_from_slice(&1080u32.to_be_bytes());
    png_bytes.extend_from_slice(&[8, 6, 0, 0, 0]);

    // 800x600 GIF header
    let mut gif_bytes = b"GIF89a".to_vec();
    gif_bytes.extend_from_slice(&800u16.to_le_bytes());
    gif_bytes.extend_from_slice(&600u16.to_le_bytes());

    c.bench_function("detect_image_dimensions_png", |b| {
        b.iter(|| {
            detect_image_dimensions(black_box(&png_bytes));
        });
    });

    c.bench_function("detect_image_dimensions_gif", |b| {
        b.iter(|| {
            detect_image_dimensions(black_box(&gif_bytes));
        });
    });
}

fn bench_quality_and_resolution_parsing(c: &mut Criterion) {
    c.bench_function("parse_dimensions_resolution_label", |b| {
        b.iter(|| {
            parse_dimensions(black_box("1080p"), black_box("1920x1080"), black_box(false));
        });
    });

    c.bench_function("parse_quality_score", |b| {
        b.iter(|| {
            parse_quality(black_box("1080p"), black_box("Download MP4 (1080p)"));
        });
    });
}

fn bench_bounded_cache(c: &mut Criterion) {
    let cache = BoundedCache::new(500, Duration::from_secs(3600));
    for i in 0..400 {
        cache.insert(format!("key_{i}"), i);
    }

    c.bench_function("bounded_cache_get_hit", |b| {
        b.iter(|| {
            cache.get(black_box("key_250"));
        });
    });

    c.bench_function("bounded_cache_insert_existing", |b| {
        b.iter(|| {
            cache.insert(black_box("key_250".to_string()), black_box(999));
        });
    });
}

criterion_group!(
    benches,
    bench_url_normalization,
    bench_duration_formatting,
    bench_image_dimension_detection,
    bench_quality_and_resolution_parsing,
    bench_bounded_cache,
);
criterion_main!(benches);
