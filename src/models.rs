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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel_id: Option<String>,
    pub author: User,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub reactions: Vec<Reaction>,
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
        }
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
