use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub avatar: Option<String>,
    #[serde(default)]
    pub status: Presence,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: "0".to_string(),
            username: "Unknown".to_string(),
            avatar: None,
            status: Presence::Offline,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Text,
    Voice,
    Category,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    pub guild_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub me: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub size: u64,
}

impl Attachment {
    /// Discord sets `content_type` for most uploads but not all of them, so the
    /// extension is the fallback.
    pub fn is_image(&self) -> bool {
        if let Some(kind) = &self.content_type {
            if kind.starts_with("image/") {
                return true;
            }
        }

        self.filename
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
    }

    /// Pixel dimensions, when Discord reported them.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match (self.width, self.height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbedMedia {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Embed {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Discord sends the accent colour as a 24-bit integer.
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub image: Option<EmbedMedia>,
    #[serde(default)]
    pub thumbnail: Option<EmbedMedia>,
}

impl Embed {
    pub fn image_url(&self) -> Option<&str> {
        self.image.as_ref().and_then(|media| media.url.as_deref())
    }

    pub fn thumbnail_url(&self) -> Option<&str> {
        self.thumbnail
            .as_ref()
            .and_then(|media| media.url.as_deref())
    }

    /// True when there is nothing to draw but an empty box.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.image_url().is_none()
            && self.thumbnail_url().is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: Option<String>,
    pub author: User,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub embeds: Vec<Embed>,
}

impl Message {
    pub fn new(author: User, content: impl Into<String>) -> Self {
        Self {
            id: Utc::now().timestamp_millis().to_string(),
            channel_id: None,
            author,
            content: content.into(),
            timestamp: Utc::now(),
            reactions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
        }
    }

    /// Every URL this message would like to display, in draw order. Used to
    /// prefetch only the rows that are on screen.
    pub fn image_urls(&self) -> impl Iterator<Item = &str> {
        self.attachments
            .iter()
            .filter(|attachment| attachment.is_image())
            .map(|attachment| attachment.url.as_str())
            .chain(self.embeds.iter().filter_map(|embed| embed.image_url()))
            .filter(|url| !url.is_empty())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Activity {
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamingState {
    pub active: bool,
    pub title: String,
    pub viewer_count: u32,
}
