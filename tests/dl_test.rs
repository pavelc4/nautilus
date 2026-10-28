use nautilus::bot::commands::dl::{format_process_duration, normalize_url};
use std::time::Duration;

#[test]
fn test_format_process_duration() {
    assert_eq!(format_process_duration(Duration::from_millis(350)), "350 ms");
    assert_eq!(format_process_duration(Duration::from_millis(1850)), "1.85s");
    assert_eq!(format_process_duration(Duration::from_secs(65)), "1m 5s");
}

#[test]
fn test_normalize_url_strips_tracking() {
    let u = "https://x.com/user/status/123?utm_source=twitter&a=1&igshid=abc";
    let norm = normalize_url(u);
    assert!(!norm.contains("utm_source"));
    assert!(!norm.contains("igshid"));
    assert!(norm.contains("a=1"));
}
