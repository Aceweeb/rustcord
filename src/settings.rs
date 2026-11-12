//! Persisted application settings.
//!
//! Every field added after the first release carries `#[serde(default = ...)]`
//! so an older `settings.json` keeps loading without a migration step. This is
//! verified by `legacy_config_still_loads`.
//!
//! Credentials are deliberately absent: the bot token lives in the OS keyring
//! (`src/auth.rs`) and is never serialised here.

use anyhow::Result;
use egui::Color32;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::shortcuts::{KeyChord, ShortcutAction};
use crate::ui::theme::{self, AppTheme, DEFAULT_THEME_ID};
use crate::utils::{color_to_hex, parse_css_color};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePriority {
    Low,
    #[default]
    Balanced,
    Performance,
}

impl ResourcePriority {
    pub const ALL: [ResourcePriority; 3] = [
        ResourcePriority::Low,
        ResourcePriority::Balanced,
        ResourcePriority::Performance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
        }
    }

    /// Minimum delay between repaint requests while the UI is idle.
    pub fn repaint_interval_ms(self) -> u64 {
        match self {
            Self::Low => 1000,
            Self::Balanced => 250,
            Self::Performance => 60,
        }
    }

    /// Message history retained in memory. Bounds peak RSS on busy gateways.
    pub fn history_cap(self) -> usize {
        match self {
            Self::Low => 500,
            Self::Balanced => 2_000,
            Self::Performance => 10_000,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "Prioritise battery and thermals. Slower refresh, small history buffer.",
            Self::Balanced => "Default. Responsive UI with a bounded message buffer.",
            Self::Performance => "Fast repaint and a large history buffer for heavy channels.",
        }
    }
}

/// Default memory budget. The brief's hard target is 500 MB total; a companion
/// that measures itself against 350 MB leaves headroom for the relay backend.
const DEFAULT_MEMORY_BUDGET_MB: u32 = 350;
const MIN_UI_SCALE: f32 = 0.8;
const MAX_UI_SCALE: f32 = 1.6;
const MIN_BUDGET_MB: u32 = 100;
const MAX_BUDGET_MB: u32 = 2000;
/// Decoded attachment/embed textures, bounded independently of the message
/// buffer so a channel full of screenshots cannot blow the RSS budget.
const DEFAULT_IMAGE_CACHE_MB: u32 = 48;
const MIN_IMAGE_CACHE_MB: u32 = 8;
const MAX_IMAGE_CACHE_MB: u32 = 512;

/// Starting budget for the image cache, in bytes.
pub const DEFAULT_IMAGE_CACHE_BYTES: u64 = DEFAULT_IMAGE_CACHE_MB as u64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // --- original keys -----------------------------------------------------
    pub vencord_themes: bool,
    pub spotify_status_sync: bool,
    pub streaming_notifications: bool,
    pub resource_priority: ResourcePriority,
    pub theme_file: Option<PathBuf>,
    pub show_blyat_banner: bool,

    // --- themes ------------------------------------------------------------
    /// Id of a built-in preset (see `ui::theme::PRESETS`).
    #[serde(default = "default_theme_preset")]
    pub theme_preset: String,
    /// `#rrggbb` accent applied on top of the preset.
    #[serde(default)]
    pub accent_override: Option<String>,

    // --- appearance --------------------------------------------------------
    #[serde(default)]
    pub compact_mode: bool,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default)]
    pub reduce_motion: bool,

    // --- memory / notifications -------------------------------------------
    #[serde(default = "default_memory_budget")]
    pub memory_budget_mb: u32,
    #[serde(default = "default_true")]
    pub memory_alerts: bool,
    #[serde(default = "default_true")]
    pub remember_history: bool,
    /// Byte budget for decoded images. Eviction is least-recently-used.
    #[serde(default = "default_image_cache_mb")]
    pub image_cache_mb: u32,

    // --- privacy / diagnostics --------------------------------------------
    #[serde(default = "default_true")]
    pub diagnostics_capture: bool,

    // --- keyboard shortcuts -------------------------------------------------
    /// Action id -> chord. Missing entries fall back to the built-in default.
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_memory_budget() -> u32 {
    DEFAULT_MEMORY_BUDGET_MB
}

fn default_image_cache_mb() -> u32 {
    DEFAULT_IMAGE_CACHE_MB
}

fn default_theme_preset() -> String {
    DEFAULT_THEME_ID.to_string()
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
            theme_preset: default_theme_preset(),
            accent_override: None,
            compact_mode: false,
            ui_scale: default_ui_scale(),
            reduce_motion: false,
            memory_budget_mb: default_memory_budget(),
            memory_alerts: true,
            remember_history: true,
            image_cache_mb: DEFAULT_IMAGE_CACHE_MB,
            diagnostics_capture: true,
            shortcuts: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustcord")
    }

    /// Local application data that is not configuration (diagnostics exports).
    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustcord")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    pub fn theme_path(&self) -> Option<PathBuf> {
        self.theme_file.clone().or_else(|| {
            let candidate = Self::config_dir().join("theme.theme");
            candidate.exists().then_some(candidate)
        })
    }

    // --- theme helpers ------------------------------------------------------

    pub fn accent(&self) -> Option<Color32> {
        self.accent_override.as_deref().and_then(parse_css_color)
    }

    pub fn set_accent(&mut self, accent: Option<Color32>) {
        self.accent_override = accent.map(color_to_hex);
    }

    pub fn accent_hex(&self) -> String {
        self.accent_override.clone().unwrap_or_default()
    }

    /// Resolve the effective palette: preset + accent override.
    pub fn theme(&self) -> AppTheme {
        AppTheme::preset(&self.theme_preset, self.accent())
    }

    pub fn theme_display_name(&self) -> &str {
        theme::preset(&self.theme_preset).name
    }

    // --- shortcuts ----------------------------------------------------------

    pub fn chord_for(&self, action: ShortcutAction) -> KeyChord {
        self.shortcuts
            .get(action.id())
            .and_then(|value| KeyChord::parse(value))
            .unwrap_or_else(|| action.default_chord())
    }

    pub fn set_chord(&mut self, action: ShortcutAction, chord: KeyChord) {
        self.shortcuts
            .insert(action.id().to_string(), chord.to_config());
    }

    pub fn reset_chord(&mut self, action: ShortcutAction) {
        self.shortcuts.remove(action.id());
    }

    pub fn reset_shortcuts(&mut self) {
        self.shortcuts.clear();
    }

    /// Pairs of actions that currently share a chord (shown in red in the UI).
    pub fn shortcut_conflicts(&self) -> Vec<(ShortcutAction, ShortcutAction)> {
        let mut conflicts = Vec::new();
        for (index, first) in ShortcutAction::ALL.iter().enumerate() {
            for second in ShortcutAction::ALL.iter().skip(index + 1) {
                if self.chord_for(*first) == self.chord_for(*second) {
                    conflicts.push((*first, *second));
                }
            }
        }
        conflicts
    }

    // --- persistence --------------------------------------------------------

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

        let mut settings = Self::load_from(&path).unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut settings: Self = serde_json::from_str(&text)?;
        settings.normalize();
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut snapshot = self.clone();
        snapshot.normalize();
        std::fs::write(path, serde_json::to_string_pretty(&snapshot)?)?;
        Ok(())
    }

    /// Clamp values that a hand-edited config could set outside a safe range.
    pub fn normalize(&mut self) {
        if !self.ui_scale.is_finite() {
            self.ui_scale = 1.0;
        }
        self.ui_scale = self.ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        self.memory_budget_mb = self.memory_budget_mb.clamp(MIN_BUDGET_MB, MAX_BUDGET_MB);
        self.image_cache_mb = self
            .image_cache_mb
            .clamp(MIN_IMAGE_CACHE_MB, MAX_IMAGE_CACHE_MB);
        if theme::preset_by(&self.theme_preset).is_none() {
            self.theme_preset = DEFAULT_THEME_ID.to_string();
        }
        if let Some(accent) = &self.accent_override {
            if parse_css_color(accent).is_none() {
                self.accent_override = None;
            }
        }
    }

    /// Debug snapshot for diagnostics exports; settings hold no secrets.
    pub fn diagnostics_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact config shape shipped by the previous session.
    const LEGACY_CONFIG: &str = r#"{
        "vencord_themes": true,
        "spotify_status_sync": true,
        "streaming_notifications": true,
        "resource_priority": "balanced",
        "theme_file": "themes/vencord-example.theme",
        "show_blyat_banner": true
    }"#;

    #[test]
    fn legacy_config_still_loads() {
        let settings: Settings = serde_json::from_str(LEGACY_CONFIG).expect("legacy parse");
        assert!(settings.vencord_themes);
        assert_eq!(settings.resource_priority, ResourcePriority::Balanced);
        // New fields fall back to their documented defaults.
        assert_eq!(settings.theme_preset, DEFAULT_THEME_ID);
        assert_eq!(settings.ui_scale, 1.0);
        assert_eq!(settings.memory_budget_mb, DEFAULT_MEMORY_BUDGET_MB);
        assert_eq!(settings.image_cache_mb, DEFAULT_IMAGE_CACHE_MB);
        assert!(settings.memory_alerts);
        assert!(settings.shortcuts.is_empty());
        assert_eq!(settings.accent(), None);
    }

    #[test]
    fn saves_and_reloads_through_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");

        let mut settings = Settings {
            theme_preset: "rosewater".to_string(),
            compact_mode: true,
            memory_budget_mb: 250,
            ..Settings::default()
        };
        settings.set_accent(Some(Color32::from_rgb(0x12, 0x34, 0x56)));
        settings.save_to(&path).expect("save");

        let loaded = Settings::load_from(&path).expect("load");
        assert_eq!(loaded.theme_preset, "rosewater");
        assert_eq!(loaded.accent(), Some(Color32::from_rgb(0x12, 0x34, 0x56)));
        assert!(loaded.compact_mode);
        assert_eq!(loaded.memory_budget_mb, 250);
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut settings = Settings {
            ui_scale: 9.0,
            memory_budget_mb: 1,
            image_cache_mb: 1,
            theme_preset: "nope".to_string(),
            accent_override: Some("not-a-colour".to_string()),
            ..Settings::default()
        };

        settings.normalize();

        assert_eq!(settings.ui_scale, MAX_UI_SCALE);
        assert_eq!(settings.memory_budget_mb, MIN_BUDGET_MB);
        assert_eq!(settings.image_cache_mb, MIN_IMAGE_CACHE_MB);
        assert_eq!(settings.theme_preset, DEFAULT_THEME_ID);
        assert_eq!(settings.accent_override, None);
    }

    #[test]
    fn accent_roundtrips_through_hex() {
        let mut settings = Settings::default();
        settings.set_accent(Some(Color32::from_rgb(0x58, 0x65, 0xf2)));
        assert_eq!(settings.accent_hex(), "#5865f2");
        assert_eq!(settings.accent(), Some(Color32::from_rgb(0x58, 0x65, 0xf2)));
        settings.set_accent(None);
        assert_eq!(settings.accent_hex(), "");
    }

    #[test]
    fn theme_resolves_preset_plus_accent() {
        let mut settings = Settings {
            theme_preset: "paper".to_string(),
            ..Settings::default()
        };
        assert_eq!(settings.theme_display_name(), "Paper");
        assert!(!settings.theme().is_dark);

        settings.set_accent(Some(Color32::from_rgb(0xff, 0x00, 0x00)));
        assert_eq!(settings.theme().accent, Color32::from_rgb(0xff, 0x00, 0x00));
    }

    #[test]
    fn shortcut_overrides_persist_and_can_be_reset() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.chord_for(ShortcutAction::RefreshView),
            ShortcutAction::RefreshView.default_chord()
        );

        settings.set_chord(
            ShortcutAction::RefreshView,
            KeyChord::new(true, false, false, "R"),
        );
        assert_eq!(
            settings.chord_for(ShortcutAction::RefreshView).to_config(),
            "Ctrl+R"
        );

        settings.reset_shortcuts();
        assert_eq!(
            settings.chord_for(ShortcutAction::RefreshView),
            ShortcutAction::RefreshView.default_chord()
        );
    }

    #[test]
    fn duplicate_shortcuts_are_reported() {
        let mut settings = Settings::default();
        let clash = KeyChord::new(true, false, false, "K");
        settings.set_chord(ShortcutAction::ToggleSettings, clash.clone());
        settings.set_chord(ShortcutAction::NextChannel, clash);

        let conflicts = settings.shortcut_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts.contains(&(ShortcutAction::ToggleSettings, ShortcutAction::NextChannel)));

        settings.reset_shortcuts();
        assert!(settings.shortcut_conflicts().is_empty());
    }

    #[test]
    fn history_caps_are_ordered_by_priority() {
        assert!(ResourcePriority::Low.history_cap() < ResourcePriority::Balanced.history_cap());
        assert!(
            ResourcePriority::Balanced.history_cap() < ResourcePriority::Performance.history_cap()
        );
        assert!(ResourcePriority::Low.repaint_interval_ms() > 60);
    }
}
