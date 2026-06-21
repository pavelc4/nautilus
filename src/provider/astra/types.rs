use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AstraResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<AstraData>,
}

#[derive(Debug, Deserialize)]
pub struct AstraData {
    pub title: Option<String>,
    pub caption: Option<String>,
    pub downloads: Option<Vec<AstraDownloadItem>>,
    pub photos: Option<Vec<AstraPhotoItem>>,
    pub videos: Option<Vec<AstraVideoItem>>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum AstraMediaType {
    Video,
    Audio,
    Image,
    Slide,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AstraDownloadItem {
    pub label: Option<String>,
    pub url: String,
    #[serde(rename = "type")]
    pub media_type: AstraMediaType,
    pub quality: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AstraPhotoItem {
    pub url: Option<String>,
    pub variants: Option<Vec<AstraPhotoVariant>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AstraPhotoVariant {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AstraVideoItem {
    pub url: String,
}

impl AstraPhotoItem {
    pub fn get_url(&self) -> Option<String> {
        if let Some(ref u) = self.url {
            return Some(u.clone());
        }
        self.variants
            .as_ref()
            .and_then(|vars| vars.first())
            .map(|v| v.url.clone())
    }
}
