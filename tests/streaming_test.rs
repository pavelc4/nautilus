use nautilus::streaming::progress::ProgressReader;
use std::io::Cursor;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn test_progress_reader_counts_bytes() {
    let data = b"hello world this is a test";
    let (mut reader, counter) = ProgressReader::new(Cursor::new(data.as_slice()));

    let mut buf = vec![0u8; data.len()];
    tokio::io::AsyncReadExt::read_exact(&mut reader, &mut buf)
        .await
        .unwrap();

    assert_eq!(counter.load(Ordering::Relaxed), data.len() as u64);
    assert_eq!(&buf, data);
}

#[tokio::test]
async fn test_progress_reader_partial_reads() {
    let data = b"abcdefghijklmnopqrstuvwxyz";
    let (mut reader, counter) = ProgressReader::new(Cursor::new(data.as_slice()));

    let mut buf = vec![0u8; 10];
    tokio::io::AsyncReadExt::read_exact(&mut reader, &mut buf)
        .await
        .unwrap();
    assert_eq!(counter.load(Ordering::Relaxed), 10);

    let mut buf2 = vec![0u8; 10];
    tokio::io::AsyncReadExt::read_exact(&mut reader, &mut buf2)
        .await
        .unwrap();
    assert_eq!(counter.load(Ordering::Relaxed), 20);
}

#[tokio::test]
async fn test_progress_reader_empty() {
    let data: &[u8] = b"";
    let (mut reader, counter) = ProgressReader::new(Cursor::new(data));

    let mut buf = vec![0u8; 4];
    let n = tokio::io::AsyncReadExt::read(&mut reader, &mut buf)
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}
