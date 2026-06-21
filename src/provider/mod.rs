pub mod astra;
pub mod registry;

use std::pin::Pin;

use async_trait::async_trait;
use tokio::io::AsyncRead;

pub type MediaReader = Pin<Box<dyn AsyncRead + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Photo,
    Audio,
    File,
}

#[derive(Debug, Clone)]
pub struct MediaMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub duration_secs: Option<u32>,
    pub dims: Option<(i32, i32)>,
    pub kind: MediaKind,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaMetadataInfo {
    pub has_video: bool,
    pub has_audio: bool,
    pub has_photo: bool,
}

pub struct MediaItem {
    pub meta: MediaMeta,
    pub reader: MediaReader,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn can_handle(&self, url: &str) -> bool;
    async fn resolve(&self, url: &str, format: Option<&str>) -> anyhow::Result<Vec<MediaItem>>;
    async fn fetch_metadata(&self, url: &str) -> anyhow::Result<MediaMetadataInfo>;
}
