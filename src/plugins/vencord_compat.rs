use crate::models::{Activity, Message, Reaction};
use crate::plugins::{Plugin, PluginContext};
use crate::settings::Settings;
use crate::ui::theme::AppTheme;
use crate::utils::parse_css_color;

/// A pragmatic subset of the Vencord/BetterDiscord theme format. We read CSS
/// custom properties and translate them into the egui palette. Arbitrary CSS
/// is never executed, and nothing is injected into the official Discord client.
///
/// Composition order matters: the user's selected preset (plus accent override)
/// is the base palette, and the `.theme` file may override individual slots on
/// top of it. A theme file that sets only `--brand-experiment` therefore keeps
/// the preset's backgrounds instead of snapping back to a hardcoded default.
pub struct VencordCompatPlugin {
    enabled: bool,
    theme: AppTheme,
    /// Alpha applied to the theme's backgrounds when a blurred wallpaper is
    /// behind the window (`DECISIONS.md` D39). `None` keeps them opaque.
    veil: Option<u8>,
}

impl Default for VencordCompatPlugin {
    fn default() -> Self {
        Self {
            enabled: true,
            theme: AppTheme::default(),
            veil: None,
        }
    }
}

impl VencordCompatPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn theme(&self) -> &AppTheme {
        &self.theme
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn reload(&mut self, settings: &Settings) {
        let base = settings.theme();
        self.enabled = settings.vencord_themes;

        let theme = if !settings.vencord_themes {
            base
        } else {
            match settings.theme_path() {
                Some(path) => match std::fs::read_to_string(&path) {
                    Ok(css) => {
                        let theme = theme_from_css(&css, base);
                        log::info!("loaded theme '{}' from {}", theme.name, path.display());
                        theme
                    }
                    Err(err) => {
                        log::warn!("could not read theme {}: {err}", path.display());
                        base
                    }
                },
                None => base,
            }
        };

        // The veil is always applied last, so a CSS theme cannot accidentally
        // paint opaque panels over the backdrop.
        self.theme = match self.veil {
            Some(alpha) => theme.veiled(alpha),
            None => theme,
        };
    }

    /// Set or clear the translucent-background alpha used while a blurred
    /// wallpaper is active, re-deriving the palette so the change is immediate.
    pub fn set_veil(&mut self, settings: &Settings, alpha: Option<u8>) {
        if self.veil == alpha {
            return;
        }
        self.veil = alpha;
        self.reload(settings);
    }
}

impl Plugin for VencordCompatPlugin {
    fn name(&self) -> &str {
        "vencord-compat"
    }

    fn init(&mut self, ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        self.reload(ctx.settings);
        Ok(())
    }

    fn on_message(&mut self, _message: &mut Message) {}

    fn on_react(&mut self, _reaction: &mut Reaction) {}

    fn on_settings(&mut self, settings: &mut Settings) {
        self.reload(settings);
    }

    fn on_activity_update(&mut self, _activity: &mut Activity) {}
}

/// Translate the supported CSS custom properties onto `base`.
fn theme_from_css(css: &str, base: AppTheme) -> AppTheme {
    let mut theme = base;
    if let Some(name) = extract_css_var(css, "--rustcord-theme-name") {
        theme.name = name.trim_matches('"').to_string();
    }

    if let Some(color) = css_var_color(css, &["--background-primary", "--background-secondary"]) {
        theme.background = color;
    }
    if let Some(color) = css_var_color(css, &["--background-secondary", "--background-primary"]) {
        theme.surface = color;
    }
    if let Some(color) = css_var_color(
        css,
        &[
            "--background-tertiary",
            "--background-secondary-alt",
            "--background-accent",
        ],
    ) {
        theme.surface_alt = color;
    }
    if let Some(color) = css_var_color(
        css,
        &["--brand-experiment", "--brand-500", "--interactive-active"],
    ) {
        theme.accent = color;
    }
    if let Some(color) = css_var_color(css, &["--text-normal", "--text-default"]) {
        theme.text = color;
    }
    if let Some(color) = css_var_color(css, &["--text-muted", "--text-subtle"]) {
        theme.muted = color;
    }
    if let Some(color) = css_var_color(css, &["--status-danger", "--red-400"]) {
        theme.danger = color;
    }
    if let Some(color) = css_var_color(css, &["--status-online", "--green-360"]) {
        theme.success = color;
    }

    theme
}

/// Read a CSS custom property value.
///
/// The property may appear anywhere on a line, so minified or single-line
/// theme files (`:root { --brand-experiment: #fff; }`) parse correctly. The
/// needle contains the colon, which prevents `--background-primary` from
/// matching `--background-primary-alt`.
fn extract_css_var(css: &str, name: &str) -> Option<String> {
    let needle = format!("{name}:");
    for line in css.lines() {
        let Some(index) = line.find(&needle) else {
            continue;
        };
        let rest = &line[index + needle.len()..];
        let value = rest.split(';').next().unwrap_or(rest);
        let value = value.trim().trim_end_matches('}').trim();
        let value = value.trim_end_matches("!important").trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn css_var_color(css: &str, names: &[&str]) -> Option<egui::Color32> {
    names
        .iter()
        .find_map(|name| extract_css_var(css, name).and_then(|value| parse_css_color(&value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: &str = r#"
        :root {
          --rustcord-theme-name: "Test Theme";
          --background-primary: #1e1f22;
          --background-secondary: #2b2d31;
          --background-tertiary: #111214;
          --brand-experiment: #5865f2;
          --text-normal: #dbdee1;
          --text-muted: #949ba4;
          --status-online: #23a55a;
          --status-danger: #f23f43;
        }
    "#;

    #[test]
    fn parses_discord_theme_variables() {
        let theme = theme_from_css(CSS, AppTheme::default());
        assert_eq!(theme.name, "Test Theme");
        assert_eq!(theme.background, egui::Color32::from_rgb(0x1e, 0x1f, 0x22));
        assert_eq!(theme.accent, egui::Color32::from_rgb(0x58, 0x65, 0xf2));
        assert_eq!(theme.text, egui::Color32::from_rgb(0xdb, 0xde, 0xe1));
    }

    #[test]
    fn missing_variables_keep_the_base_palette() {
        let base = AppTheme::preset("rosewater", None);
        let theme = theme_from_css("body { color: red; }", base);
        assert_eq!(
            theme.background,
            AppTheme::preset("rosewater", None).background
        );
        assert_eq!(theme.accent, AppTheme::preset("rosewater", None).accent);
        assert!(theme.name.contains("Rosewater") || theme.name == "Rosewater");
    }

    #[test]
    fn css_overrides_only_the_keys_it_defines() {
        let base = AppTheme::preset("paper", None);
        let base_text = base.text;
        let theme = theme_from_css(":root { --brand-experiment: #ff0000; }", base);

        assert_eq!(theme.accent, egui::Color32::from_rgb(255, 0, 0));
        // Everything else still comes from the (light) preset.
        assert_eq!(theme.text, base_text);
        assert!(
            !theme.is_dark,
            "preset polarity must survive a CSS override"
        );
    }

    #[test]
    fn strips_important_suffix() {
        assert_eq!(
            extract_css_var("--text-normal: #ffffff !important;", "--text-normal"),
            Some("#ffffff".to_string())
        );
    }
}
