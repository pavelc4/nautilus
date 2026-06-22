use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use pin_project::pin_project;
use tokio::io::{AsyncRead, ReadBuf};

#[pin_project]
pub struct ProgressReader<R> {
    #[pin]
    inner: R,
    counter: Arc<AtomicU64>,
}

impl<R> ProgressReader<R> {
    pub fn new(inner: R) -> (Self, Arc<AtomicU64>) {
        let counter = Arc::new(AtomicU64::new(0));
        (
            Self {
                inner,
                counter: counter.clone(),
            },
            counter,
        )
    }
}

impl<R: AsyncRead> AsyncRead for ProgressReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.project();
        let filled_before = buf.filled().len();
        let result = this.inner.poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let bytes_read = buf.filled().len() - filled_before;
            this.counter.fetch_add(bytes_read as u64, Ordering::Relaxed);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
}
