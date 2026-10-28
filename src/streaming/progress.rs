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
