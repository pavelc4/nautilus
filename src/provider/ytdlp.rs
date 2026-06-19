use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::provider::{MediaKind, MediaMeta, MediaReader, Provider};

pub struct YtDlpProvider {
    cookies: Option<String>,
}

impl YtDlpProvider {
    pub fn new(cookies: Option<String>) -> Self {
        Self { cookies }
    }

    fn build_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("yt-dlp");
        cmd.args(args);
        if let Some(ref cookies) = self.cookies {
            cmd.arg("--cookies").arg(cookies);
        }
        cmd
    }
}

#[async_trait]
impl Provider for YtDlpProvider {
    fn can_handle(&self, url: &str) -> bool {
        url.contains("youtube.com")
            || url.contains("youtu.be")
            || url.contains("yewtu.be")
            || url.contains("inv.nadeko.net")
    }

    async fn resolve(&self, url: &str) -> anyhow::Result<(MediaMeta, MediaReader)> {
        let mut probe = self.build_command(&[
            "--print",
            "%(filesize,filesize_approx)s",
            "--print",
            "%(ext)s",
            "--print",
            "%(duration)s",
            "--print",
            "%(width)s",
            "--print",
            "%(height)s",
            "--print",
            "%(display_id)s",
            "--no-download",
            "--no-warnings",
            url,
        ]);
        let output = probe.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("yt-dlp probe failed: {stderr}");
        }
        let mut lines = BufReader::new(&output.stdout[..]).lines();
        let size_str = lines.next_line().await?.unwrap_or_default();
        let ext = lines
            .next_line()
            .await?
            .unwrap_or_else(|| String::from("mp4"));
        let duration_str = lines.next_line().await?.unwrap_or_default();
        let width_str = lines.next_line().await?.unwrap_or_default();
        let height_str = lines.next_line().await?.unwrap_or_default();
        let display_id = lines.next_line().await?.unwrap_or_default();

        let size: u64 = size_str.trim().parse().unwrap_or(0);
        let duration_secs: Option<u32> = duration_str.trim().parse().ok();
        let width: i32 = width_str.trim().parse().unwrap_or(0);
        let height: i32 = height_str.trim().parse().unwrap_or(0);

        let filename = if display_id.is_empty() {
            format!("video.{ext}")
        } else {
            format!("{display_id}.{ext}")
        };

        let kind = MediaKind::Video;
        let mime_type = mime_for_ext(&ext);
        let dims = if width > 0 && height > 0 {
            Some((width, height))
        } else {
            None
        };

        let meta = MediaMeta {
            filename,
            mime_type,
            size,
            duration_secs,
            dims,
            kind,
        };

        let stream = self.stream(url).await?;
        Ok((meta, stream))
    }
}

impl YtDlpProvider {
    async fn stream(&self, url: &str) -> anyhow::Result<MediaReader> {
        let mut child = self
            .build_command(&["-o", "-", "--no-warnings", url])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdout from yt-dlp"))?;

        tokio::spawn(async move {
            let status = child.wait().await;
            if let Ok(status) = status
                && !status.success()
            {
                tracing::warn!("yt-dlp exited with: {status}");
            }
        });

        Ok(Box::pin(stdout))
    }
}

fn mime_for_ext(ext: &str) -> String {
    match ext {
        "mp4" | "m4v" => "video/mp4".into(),
        "webm" => "video/webm".into(),
        "mkv" => "video/x-matroska".into(),
        "avi" => "video/x-msvideo".into(),
        "mov" => "video/quicktime".into(),
        "mp3" => "audio/mpeg".into(),
        "m4a" => "audio/mp4".into(),
        "ogg" => "audio/ogg".into(),
        "wav" => "audio/wav".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "png" => "image/png".into(),
        "gif" => "image/gif".into(),
        _ => "application/octet-stream".into(),
    }
}
