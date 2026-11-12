use crate::models::{Activity, Message, Reaction};
use crate::plugins::{Plugin, PluginContext};
use crate::settings::Settings;
use crate::ui::theme::AppTheme;
use crate::utils::parse_css_color;

/// A pragmatic subset of the Vencord/BetterDiscord theme format. We read CSS
/// custom properties and translate them into the egui palette. Arbitrary CSS
/// is never executed.
pub struct VencordCompatPlugin {
    enabled: bool,
    theme: AppTheme,
}

impl Default for VencordCompatPlugin {
    fn default() -> Self {
        Self {
            enabled: true,
            theme: AppTheme::default(),
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
        if !settings.vencord_themes {
            self.enabled = false;
            self.theme = AppTheme::default();
            return;
        }

        self.enabled = true;
        let path = settings.theme_file.clone().or_else(|| {
            let candidate = Settings::config_path().with_file_name("theme.theme");
            candidate.exists().then_some(candidate)
        });

        let Some(path) = path else {
            self.theme = AppTheme::default();
            return;
        };

        match std::fs::read_to_string(&path) {
            Ok(css) => {
                self.theme = theme_from_css(&css);
                log::info!("loaded theme '{}' from {}", self.theme.name, path.display());
            }
            Err(err) => {
                log::warn!("could not read theme {}: {err}", path.display());
                self.theme = AppTheme::default();
            }
        }
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

fn theme_from_css(css: &str) -> AppTheme {
    let mut theme = AppTheme::default();
    theme.name = extract_css_var(css, "--rustcord-theme-name").unwrap_or(theme.name);

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

fn extract_css_var(css: &str, name: &str) -> Option<String> {
    let needle = format!("{name}:");
    for line in css.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&needle) {
            let value = rest
                .trim()
                .trim_end_matches(';')
                .trim()
                .trim_end_matches("!important")
                .trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
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

    #[test]
    fn parses_discord_theme_variables() {
        let css = r#"
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

        let theme = theme_from_css(css);
        assert_eq!(theme.name, "\"Test Theme\"");
        assert_eq!(theme.background, egui::Color32::from_rgb(0x1e, 0x1f, 0x22));
        assert_eq!(theme.accent, egui::Color32::from_rgb(0x58, 0x65, 0xf2));
        assert_eq!(theme.text, egui::Color32::from_rgb(0xdb, 0xde, 0xe1));
    }

    #[test]
    fn missing_variables_keep_defaults() {
        let theme = theme_from_css("body { color: red; }");
        assert_eq!(theme.background, AppTheme::default().background);
        assert_eq!(theme.accent, AppTheme::default().accent);
    }

    #[test]
    fn strips_important_suffix() {
        assert_eq!(
            extract_css_var("--text-normal: #ffffff !important;", "--text-normal"),
            Some("#ffffff".to_string())
        );
    }
}
