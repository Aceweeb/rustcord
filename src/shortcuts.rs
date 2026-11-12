//! Keyboard shortcut model: parseable, persistable chords plus the action list.
//!
//! Chords are stored as strings (`"Ctrl+Shift+C"`) in `settings.json`, which
//! keeps the config human-editable and forward-compatible. Matching against
//! egui events goes through the key's `Debug` name so a rename cannot silently
//! break a binding without failing the parse tests.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    Search,
    QuickSwitcher,
    ToggleSettings,
    NextChannel,
    PrevChannel,
    NextGuild,
    PrevGuild,
    RefreshView,
    ToggleCompact,
    ExportDiagnostics,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 10] = [
        ShortcutAction::Search,
        ShortcutAction::QuickSwitcher,
        ShortcutAction::ToggleSettings,
        ShortcutAction::NextChannel,
        ShortcutAction::PrevChannel,
        ShortcutAction::NextGuild,
        ShortcutAction::PrevGuild,
        ShortcutAction::RefreshView,
        ShortcutAction::ToggleCompact,
        ShortcutAction::ExportDiagnostics,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::QuickSwitcher => "quick_switcher",
            Self::ToggleSettings => "toggle_settings",
            Self::NextChannel => "next_channel",
            Self::PrevChannel => "prev_channel",
            Self::NextGuild => "next_guild",
            Self::PrevGuild => "prev_guild",
            Self::RefreshView => "refresh_view",
            Self::ToggleCompact => "toggle_compact",
            Self::ExportDiagnostics => "export_diagnostics",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "Search loaded messages",
            Self::QuickSwitcher => "Quick switcher (servers and channels)",
            Self::ToggleSettings => "Open / close settings",
            Self::NextChannel => "Next channel",
            Self::PrevChannel => "Previous channel",
            Self::NextGuild => "Next server",
            Self::PrevGuild => "Previous server",
            Self::RefreshView => "Reload theme file",
            Self::ToggleCompact => "Toggle compact mode",
            Self::ExportDiagnostics => "Export diagnostics",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    pub fn default_chord(self) -> KeyChord {
        match self {
            Self::Search => KeyChord::new(true, false, false, "F"),
            Self::QuickSwitcher => KeyChord::new(true, false, false, "K"),
            Self::ToggleSettings => KeyChord::new(true, false, false, "Comma"),
            Self::NextChannel => KeyChord::new(true, false, false, "ArrowDown"),
            Self::PrevChannel => KeyChord::new(true, false, false, "ArrowUp"),
            Self::NextGuild => KeyChord::new(false, false, true, "ArrowDown"),
            Self::PrevGuild => KeyChord::new(false, false, true, "ArrowUp"),
            Self::RefreshView => KeyChord::new(false, false, false, "F5"),
            Self::ToggleCompact => KeyChord::new(true, true, false, "C"),
            Self::ExportDiagnostics => KeyChord::new(true, true, false, "D"),
        }
    }
}

/// egui key names for punctuation, mapped to their human-readable glyph.
const PUNCTUATION: &[(&str, &str)] = &[
    ("Comma", ","),
    ("Period", "."),
    ("Slash", "/"),
    ("Backslash", "\\"),
    ("Minus", "-"),
    ("Equals", "="),
    ("Semicolon", ";"),
    ("Quote", "'"),
    ("Backtick", "`"),
    ("OpenBracket", "["),
    ("CloseBracket", "]"),
];

/// Named keys accepted in a chord, besides single letters and digits.
const NAMED_KEYS: &[&str] = &[
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Escape",
    "Enter",
    "Tab",
    "Space",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// egui-style key name, e.g. `"Comma"`, `"F5"`, `"C"`.
    pub key: String,
}

impl KeyChord {
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: impl Into<String>) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key: key.into(),
        }
    }

    /// Match an egui key event. `ctrl` should include `modifiers.command` so
    /// the binding also works on macOS-style keyboards.
    pub fn matches(&self, key_name: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.ctrl == ctrl && self.shift == shift && self.alt == alt && self.key == key_name
    }

    pub fn display_key(&self) -> String {
        PUNCTUATION
            .iter()
            .find(|(name, _)| *name == self.key)
            .map(|(_, glyph)| (*glyph).to_string())
            .unwrap_or_else(|| self.key.clone())
    }

    /// Canonical, parseable representation (`"Ctrl+Shift+C"`).
    pub fn to_config(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(self.display_key());
        parts.join("+")
    }

    pub fn parse(input: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key: Option<String> = None;

        for raw in input.split('+') {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "super" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                other => {
                    if key.is_some() {
                        // More than one non-modifier key: ambiguous, reject.
                        return None;
                    }
                    key = Some(canonical_key_name(other)?);
                }
            }
        }

        Some(Self {
            ctrl,
            shift,
            alt,
            key: key?,
        })
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_config())
    }
}

fn canonical_key_name(input: &str) -> Option<String> {
    // Punctuation glyphs first.
    if let Some((name, _)) = PUNCTUATION.iter().find(|(_, glyph)| *glyph == input) {
        return Some((*name).to_string());
    }

    let lower = input.to_ascii_lowercase();
    for name in NAMED_KEYS {
        if name.to_ascii_lowercase() == lower {
            return Some((*name).to_string());
        }
        // Accept `Up` / `Down` as shorthands for the arrow keys.
        if name.starts_with("Arrow") && name[5..].to_ascii_lowercase() == lower {
            return Some((*name).to_string());
        }
    }

    let mut chars = input.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(ch.to_ascii_uppercase().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_combinations() {
        let chord = KeyChord::parse("ctrl+shift+c").expect("parse");
        assert_eq!(chord, KeyChord::new(true, true, false, "C"));
        assert_eq!(chord.to_config(), "Ctrl+Shift+C");
    }

    #[test]
    fn parses_single_keys_and_functions() {
        assert_eq!(
            KeyChord::parse("F5"),
            Some(KeyChord::new(false, false, false, "F5"))
        );
        assert_eq!(
            KeyChord::parse("alt+up"),
            Some(KeyChord::new(false, false, true, "ArrowUp"))
        );
    }

    #[test]
    fn parses_punctuation_glyphs() {
        let chord = KeyChord::parse("Ctrl+,").expect("parse");
        assert_eq!(chord.key, "Comma");
        assert_eq!(chord.to_config(), "Ctrl+,");
    }

    #[test]
    fn rejects_unknown_and_ambiguous_chords() {
        assert!(KeyChord::parse("Ctrl+Frobnicate").is_none());
        assert!(KeyChord::parse("Ctrl+A+B").is_none());
        assert!(KeyChord::parse("Ctrl+").is_none());
    }

    #[test]
    fn every_default_roundtrips_through_the_config_string() {
        for action in ShortcutAction::ALL {
            let chord = action.default_chord();
            let text = chord.to_config();
            let parsed = KeyChord::parse(&text)
                .unwrap_or_else(|| panic!("{} did not roundtrip ({text})", action.id()));
            assert_eq!(parsed, chord, "{} changed during roundtrip", action.id());
        }
    }

    #[test]
    fn matching_requires_exact_modifiers() {
        let chord = KeyChord::new(true, false, false, "K");
        assert!(chord.matches("K", true, false, false));
        assert!(!chord.matches("K", true, true, false));
        assert!(!chord.matches("K", false, false, false));
        assert!(!chord.matches("J", true, false, false));
    }

    #[test]
    fn action_ids_are_unique_and_roundtrip() {
        let mut ids: Vec<_> = ShortcutAction::ALL
            .iter()
            .map(|action| action.id())
            .collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
        for action in ShortcutAction::ALL {
            assert_eq!(ShortcutAction::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn default_chords_do_not_collide() {
        let mut chords: Vec<String> = ShortcutAction::ALL
            .iter()
            .map(|action| action.default_chord().to_config())
            .collect();
        chords.sort();
        let count = chords.len();
        chords.dedup();
        assert_eq!(chords.len(), count, "duplicate default shortcut");
    }
}
