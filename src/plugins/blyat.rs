use crate::models::{Activity, Message, Reaction};
use crate::plugins::{Plugin, PluginContext};
use crate::settings::Settings;

/// Tiny example plugin from the brief: shows a toggleable banner at the bottom
/// of the channel list. The banner content lives here so it can later be
/// replaced by a WASM plugin without touching the UI code.
pub struct BlyatPlugin {
    enabled: bool,
}

impl Default for BlyatPlugin {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl BlyatPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn banner_text(&self) -> Option<&'static str> {
        self.enabled.then_some("Blyat! 🍻  ·  hello from a plugin")
    }
}

impl Plugin for BlyatPlugin {
    fn name(&self) -> &str {
        "blyat-banner"
    }

    fn init(&mut self, ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        self.enabled = ctx.settings.show_blyat_banner;
        Ok(())
    }

    fn on_message(&mut self, _message: &mut Message) {}

    fn on_react(&mut self, _reaction: &mut Reaction) {}

    fn on_settings(&mut self, settings: &mut Settings) {
        self.enabled = settings.show_blyat_banner;
    }

    fn on_activity_update(&mut self, _activity: &mut Activity) {}
}
