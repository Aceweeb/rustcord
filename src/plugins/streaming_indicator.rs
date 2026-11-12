use crate::models::{Activity, Message, Reaction, StreamingState};
use crate::plugins::{Plugin, PluginContext};
use crate::settings::Settings;

pub struct StreamingIndicatorPlugin {
    enabled: bool,
    state: StreamingState,
}

impl Default for StreamingIndicatorPlugin {
    fn default() -> Self {
        Self {
            enabled: true,
            state: StreamingState::default(),
        }
    }
}

impl StreamingIndicatorPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &StreamingState {
        &self.state
    }

    pub fn start(&mut self, title: impl Into<String>, viewers: u32) {
        self.state.active = true;
        self.state.title = title.into();
        self.state.viewer_count = viewers;
    }

    pub fn stop(&mut self) {
        self.state.active = false;
        self.state.title.clear();
        self.state.viewer_count = 0;
    }
}

impl Plugin for StreamingIndicatorPlugin {
    fn name(&self) -> &str {
        "streaming-indicator"
    }

    fn init(&mut self, ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        self.enabled = ctx.settings.streaming_notifications;
        if self.enabled {
            self.start("Rustcord dev stream", 12);
        }
        Ok(())
    }

    fn on_message(&mut self, _message: &mut Message) {}

    fn on_react(&mut self, _reaction: &mut Reaction) {}

    fn on_settings(&mut self, settings: &mut Settings) {
        self.enabled = settings.streaming_notifications;
        if !self.enabled {
            self.stop();
        }
    }

    fn on_activity_update(&mut self, _activity: &mut Activity) {}

    fn on_streaming_update(&mut self, active: bool, title: &str) {
        if active {
            self.start(title.to_string(), self.state.viewer_count);
        } else {
            self.stop();
        }
    }
}
