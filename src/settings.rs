use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePriority {
    Low,
    #[default]
    Balanced,
    Performance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub vencord_themes: bool,
    pub spotify_status_sync: bool,
    pub streaming_notifications: bool,
    pub resource_priority: ResourcePriority,
    pub theme_file: Option<PathBuf>,
    pub show_blyat_banner: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            vencord_themes: true,
            spotify_status_sync: true,
            streaming_notifications: true,
            resource_priority: ResourcePriority::Balanced,
            theme_file: None,
            show_blyat_banner: true,
        }
    }
}

impl Settings {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustcord")
            .join("settings.json")
    }

    pub fn load_or_default() -> Self {
        let config_path = Self::config_path();
        let local_path = PathBuf::from("settings.json");
        let path = if config_path.exists() {
            config_path
        } else if local_path.exists() {
            local_path
        } else {
            config_path
        };

        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(settings) => settings,
                Err(err) => {
                    log::warn!("could not parse {}: {err}; using defaults", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
