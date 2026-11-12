//! Theme engine: built-in presets plus the live egui `Visuals` application.
//!
//! Three layers exist, in increasing priority:
//!   1. a built-in preset (`PRESETS`) — the baseline palette,
//!   2. an optional user accent colour that replaces only the accent slot,
//!   3. the Vencord-compatible `.theme` file, which may override any slot.
//!
//! Layer 3 is applied by `plugins::vencord_compat`, which composes on top of
//! the palette produced here. Nothing in this module ever injects CSS or code
//! into the official Discord client; it only styles this process' own window.

use egui::{Color32, Stroke};

#[derive(Debug, Clone)]
pub struct AppTheme {
    pub name: String,
    /// Drives the base egui `Visuals` and the built-in widget polish.
    pub is_dark: bool,
    pub background: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub danger: Color32,
    pub success: Color32,
}

impl Default for AppTheme {
    fn default() -> Self {
        Self::preset(DEFAULT_THEME_ID, None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeCategory {
    Dark,
    Light,
    Purple,
    Amoled,
}

impl ThemeCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Purple => "Purple",
            Self::Amoled => "AMOLED",
        }
    }

    pub const ALL: [ThemeCategory; 4] = [
        ThemeCategory::Dark,
        ThemeCategory::Light,
        ThemeCategory::Purple,
        ThemeCategory::Amoled,
    ];
}

/// `Copy` palette: cheap enough to build every frame for the gallery previews.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub danger: Color32,
    pub success: Color32,
}

pub struct ThemePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub category: ThemeCategory,
    pub palette: Palette,
}

/// Const colour constructor so the preset table stays in `.rodata`.
const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const DEFAULT_THEME_ID: &str = "midnight";

pub const PRESETS: &[ThemePreset] = &[
    ThemePreset {
        id: "midnight",
        name: "Midnight",
        category: ThemeCategory::Dark,
        palette: Palette {
            background: c(0x1b, 0x1d, 0x21),
            surface: c(0x23, 0x25, 0x29),
            surface_alt: c(0x12, 0x13, 0x16),
            accent: c(0x58, 0x65, 0xf2),
            text: c(0xe6, 0xe8, 0xec),
            muted: c(0x9a, 0xa0, 0xaa),
            danger: c(0xf0, 0x56, 0x5c),
            success: c(0x2f, 0xd0, 0x7a),
        },
    },
    ThemePreset {
        id: "graphite",
        name: "Graphite",
        category: ThemeCategory::Dark,
        palette: Palette {
            background: c(0x20, 0x22, 0x25),
            surface: c(0x2b, 0x2d, 0x31),
            surface_alt: c(0x16, 0x18, 0x1a),
            accent: c(0x3b, 0xa5, 0x5d),
            text: c(0xdb, 0xde, 0xe1),
            muted: c(0x94, 0x9b, 0xa4),
            danger: c(0xf2, 0x3f, 0x43),
            success: c(0x23, 0xa5, 0x5a),
        },
    },
    ThemePreset {
        id: "slate",
        name: "Slate",
        category: ThemeCategory::Dark,
        palette: Palette {
            background: c(0x1c, 0x24, 0x30),
            surface: c(0x25, 0x32, 0x43),
            surface_alt: c(0x14, 0x1b, 0x24),
            accent: c(0x4e, 0xa8, 0xde),
            text: c(0xe3, 0xea, 0xf2),
            muted: c(0x9a, 0xa9, 0xba),
            danger: c(0xef, 0x6b, 0x6b),
            success: c(0x4f, 0xd1, 0x8a),
        },
    },
    ThemePreset {
        id: "forest",
        name: "Forest",
        category: ThemeCategory::Dark,
        palette: Palette {
            background: c(0x12, 0x1a, 0x15),
            surface: c(0x1a, 0x26, 0x20),
            surface_alt: c(0x0c, 0x12, 0x0e),
            accent: c(0x3d, 0xdc, 0x97),
            text: c(0xe2, 0xef, 0xe8),
            muted: c(0x93, 0xab, 0x9f),
            danger: c(0xe8, 0x6a, 0x5c),
            success: c(0x3d, 0xdc, 0x97),
        },
    },
    ThemePreset {
        id: "ember",
        name: "Ember",
        category: ThemeCategory::Dark,
        palette: Palette {
            background: c(0x1f, 0x15, 0x12),
            surface: c(0x2a, 0x1d, 0x18),
            surface_alt: c(0x15, 0x0e, 0x0b),
            accent: c(0xff, 0x70, 0x43),
            text: c(0xf3, 0xe6, 0xdf),
            muted: c(0xb3, 0x9a, 0x8d),
            danger: c(0xff, 0x5a, 0x52),
            success: c(0x5f, 0xd3, 0x8b),
        },
    },
    ThemePreset {
        id: "obsidian",
        name: "Obsidian",
        category: ThemeCategory::Amoled,
        palette: Palette {
            background: c(0x00, 0x00, 0x00),
            surface: c(0x08, 0x09, 0x0a),
            surface_alt: c(0x10, 0x12, 0x14),
            accent: c(0x7c, 0x8c, 0xff),
            text: c(0xf2, 0xf3, 0xf5),
            muted: c(0x9a, 0xa0, 0xaa),
            danger: c(0xff, 0x5c, 0x5c),
            success: c(0x2f, 0xd0, 0x7a),
        },
    },
    ThemePreset {
        id: "void",
        name: "Void",
        category: ThemeCategory::Amoled,
        palette: Palette {
            background: c(0x00, 0x00, 0x00),
            surface: c(0x05, 0x05, 0x05),
            surface_alt: c(0x0f, 0x0f, 0x0f),
            accent: c(0xeb, 0x45, 0x9b),
            text: c(0xfa, 0xfa, 0xfa),
            muted: c(0xa8, 0xa8, 0xa8),
            danger: c(0xff, 0x55, 0x55),
            success: c(0x00, 0xd1, 0x7a),
        },
    },
    ThemePreset {
        id: "amethyst",
        name: "Amethyst",
        category: ThemeCategory::Purple,
        palette: Palette {
            background: c(0x1a, 0x14, 0x25),
            surface: c(0x24, 0x1a, 0x33),
            surface_alt: c(0x12, 0x0d, 0x1a),
            accent: c(0xa8, 0x55, 0xf7),
            text: c(0xec, 0xe6, 0xf5),
            muted: c(0xa9, 0x9b, 0xc0),
            danger: c(0xf0, 0x56, 0x5c),
            success: c(0x57, 0xd9, 0x9f),
        },
    },
    ThemePreset {
        id: "ultraviolet",
        name: "Ultraviolet",
        category: ThemeCategory::Purple,
        palette: Palette {
            background: c(0x00, 0x00, 0x00),
            surface: c(0x0d, 0x07, 0x16),
            surface_alt: c(0x16, 0x0c, 0x24),
            accent: c(0xb5, 0x7b, 0xff),
            text: c(0xf4, 0xef, 0xff),
            muted: c(0xa6, 0x95, 0xc4),
            danger: c(0xff, 0x60, 0x6a),
            success: c(0x57, 0xd9, 0x9f),
        },
    },
    ThemePreset {
        id: "paper",
        name: "Paper",
        category: ThemeCategory::Light,
        palette: Palette {
            background: c(0xff, 0xff, 0xff),
            surface: c(0xf2, 0xf3, 0xf5),
            surface_alt: c(0xe4, 0xe6, 0xea),
            accent: c(0x47, 0x52, 0xc4),
            text: c(0x1e, 0x21, 0x24),
            muted: c(0x5c, 0x64, 0x70),
            danger: c(0xc4, 0x2b, 0x2b),
            success: c(0x1a, 0x7f, 0x4b),
        },
    },
    ThemePreset {
        id: "parchment",
        name: "Parchment",
        category: ThemeCategory::Light,
        palette: Palette {
            background: c(0xfa, 0xf6, 0xef),
            surface: c(0xf1, 0xea, 0xe0),
            surface_alt: c(0xe6, 0xdc, 0xcc),
            accent: c(0xa8, 0x4b, 0x08),
            text: c(0x2b, 0x20, 0x18),
            muted: c(0x6b, 0x5a, 0x49),
            danger: c(0xb3, 0x2b, 0x2b),
            success: c(0x2f, 0x6b, 0x3f),
        },
    },
    ThemePreset {
        id: "rosewater",
        name: "Rosewater",
        category: ThemeCategory::Light,
        palette: Palette {
            background: c(0xff, 0xf5, 0xf7),
            surface: c(0xff, 0xe9, 0xee),
            surface_alt: c(0xf8, 0xd7, 0xe0),
            accent: c(0xc2, 0x2b, 0x5f),
            text: c(0x2a, 0x12, 0x19),
            muted: c(0x7a, 0x4a, 0x58),
            danger: c(0xb3, 0x1f, 0x2f),
            success: c(0x1f, 0x6f, 0x4a),
        },
    },
];

/// Look up a preset, falling back to the default one for unknown ids.
pub fn preset(id: &str) -> &'static ThemePreset {
    PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .unwrap_or(&PRESETS[0])
}

pub fn preset_by(id: &str) -> Option<&'static ThemePreset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

impl AppTheme {
    pub fn from_preset(preset: &ThemePreset, accent: Option<Color32>) -> Self {
        let palette = preset.palette;
        Self {
            name: preset.name.to_string(),
            is_dark: preset.category != ThemeCategory::Light,
            background: palette.background,
            surface: palette.surface,
            surface_alt: palette.surface_alt,
            accent: accent.unwrap_or(palette.accent),
            text: palette.text,
            muted: palette.muted,
            danger: palette.danger,
            success: palette.success,
        }
    }

    pub fn preset(id: &str, accent: Option<Color32>) -> Self {
        Self::from_preset(preset(id), accent)
    }

    /// Darken/lighten a slot without leaving the palette's contrast budget.
    pub fn with_accent(mut self, accent: Option<Color32>) -> Self {
        if let Some(accent) = accent {
            self.accent = accent;
        }
        self
    }

    /// Apply the theme to the egui context.
    ///
    /// Called every frame by the app so live theme switches and `.theme` file
    /// reloads are visible immediately. It only writes a handful of colours, so
    /// the cost is negligible compared to a redraw.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.surface_alt;
        visuals.faint_bg_color = self.surface_alt;
        visuals.code_bg_color = self.surface_alt;
        visuals.override_text_color = Some(self.text);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.danger;
        visuals.error_fg_color = self.danger;

        visuals.widgets.noninteractive.bg_fill = self.surface;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.surface_alt);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.muted);

        visuals.widgets.inactive.bg_fill = self.surface_alt;
        visuals.widgets.inactive.weak_bg_fill = self.surface;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);

        visuals.widgets.hovered.bg_fill = self.surface_alt;
        visuals.widgets.hovered.weak_bg_fill = self.accent.gamma_multiply(0.35);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, self.text);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5, crate::utils::readable_on(self.accent));

        visuals.selection.bg_fill = self.accent.gamma_multiply(0.55);
        visuals.selection.stroke = Stroke::new(1.0, self.text);
        visuals.window_stroke = Stroke::new(1.0, self.surface_alt);

        ctx.set_visuals(visuals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::contrast_ratio;

    #[test]
    fn preset_ids_are_unique() {
        let mut ids: Vec<_> = PRESETS.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate preset id");
    }

    #[test]
    fn covers_every_required_category() {
        for category in ThemeCategory::ALL {
            assert!(
                PRESETS.iter().any(|preset| preset.category == category),
                "no preset for {category:?}"
            );
        }
    }

    #[test]
    fn text_contrast_meets_wcag_aa() {
        for preset in PRESETS {
            let palette = preset.palette;
            let body = contrast_ratio(palette.text, palette.background);
            let muted = contrast_ratio(palette.muted, palette.background);
            assert!(
                body >= 4.5,
                "{} body text contrast {body:.2} < 4.5",
                preset.name
            );
            assert!(
                muted >= 3.0,
                "{} muted text contrast {muted:.2} < 3.0",
                preset.name
            );
            let accent = contrast_ratio(palette.accent, palette.background);
            assert!(
                accent >= 2.0,
                "{} accent contrast {accent:.2} < 2.0",
                preset.name
            );
        }
    }

    #[test]
    fn light_presets_are_flagged_light() {
        for preset in PRESETS {
            let theme = AppTheme::from_preset(preset, None);
            assert_eq!(
                theme.is_dark,
                preset.category != ThemeCategory::Light,
                "{} is_dark mismatch",
                preset.name
            );
        }
    }

    #[test]
    fn accent_override_only_changes_the_accent() {
        let base = AppTheme::preset("paper", None);
        let accent = Color32::from_rgb(0x99, 0x00, 0xff);
        let themed = AppTheme::preset("paper", Some(accent));
        assert_eq!(themed.accent, accent);
        assert_eq!(themed.text, base.text);
        assert_eq!(themed.background, base.background);
        assert_eq!(themed.surface, base.surface);
    }

    #[test]
    fn unknown_preset_falls_back_to_default() {
        assert_eq!(preset("does-not-exist").id, DEFAULT_THEME_ID);
        assert_eq!(AppTheme::default().name, preset(DEFAULT_THEME_ID).name);
        assert!(preset_by("does-not-exist").is_none());
    }
}
