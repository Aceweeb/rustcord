use crate::models::{Activity, Message, Reaction, StreamingState};
use crate::settings::Settings;
use crate::ui::theme::AppTheme;

pub mod blyat;
pub mod spotify_status;
pub mod streaming_indicator;
pub mod vencord_compat;

pub struct PluginContext<'a> {
    pub settings: &'a mut Settings,
}

/// Contract for external/native plugins. Built-in plugins also implement it,
/// but the manager holds them concretely so the UI can query typed state
/// without downcasting.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    fn init(&mut self, _ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_message(&mut self, _message: &mut Message) {}

    fn on_react(&mut self, _reaction: &mut Reaction) {}

    fn on_settings(&mut self, _settings: &mut Settings) {}

    fn on_activity_update(&mut self, _activity: &mut Activity) {}

    fn on_streaming_update(&mut self, _active: bool, _title: &str) {}
}

pub struct PluginManager {
    pub vencord: vencord_compat::VencordCompatPlugin,
    pub spotify: spotify_status::SpotifyStatusPlugin,
    pub streaming: streaming_indicator::StreamingIndicatorPlugin,
    pub blyat: blyat::BlyatPlugin,
    external: Vec<Box<dyn Plugin>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            vencord: vencord_compat::VencordCompatPlugin::new(),
            spotify: spotify_status::SpotifyStatusPlugin::new(),
            streaming: streaming_indicator::StreamingIndicatorPlugin::new(),
            blyat: blyat::BlyatPlugin::new(),
            external: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.external.push(plugin);
    }

    pub fn init_all(&mut self, settings: &mut Settings) {
        init_one(&mut self.vencord, settings);
        init_one(&mut self.spotify, settings);
        init_one(&mut self.streaming, settings);
        init_one(&mut self.blyat, settings);

        for plugin in &mut self.external {
            let mut ctx = PluginContext {
                settings: &mut *settings,
            };
            if let Err(err) = plugin.init(&mut ctx) {
                log::warn!("plugin '{}' failed to initialize: {err:#}", plugin.name());
            }
        }
    }

    pub fn on_message(&mut self, message: &mut Message) {
        self.vencord.on_message(message);
        self.spotify.on_message(message);
        self.streaming.on_message(message);
        self.blyat.on_message(message);
        for plugin in &mut self.external {
            plugin.on_message(message);
        }
    }

    pub fn on_react(&mut self, reaction: &mut Reaction) {
        self.vencord.on_react(reaction);
        self.spotify.on_react(reaction);
        self.streaming.on_react(reaction);
        self.blyat.on_react(reaction);
        for plugin in &mut self.external {
            plugin.on_react(reaction);
        }
    }

    pub fn on_settings(&mut self, settings: &mut Settings) {
        self.vencord.on_settings(settings);
        self.spotify.on_settings(settings);
        self.streaming.on_settings(settings);
        self.blyat.on_settings(settings);
        for plugin in &mut self.external {
            plugin.on_settings(settings);
        }
    }

    pub fn on_activity_update(&mut self, activity: &mut Activity) {
        self.vencord.on_activity_update(activity);
        self.spotify.on_activity_update(activity);
        self.streaming.on_activity_update(activity);
        self.blyat.on_activity_update(activity);
        for plugin in &mut self.external {
            plugin.on_activity_update(activity);
        }
    }

    pub fn on_streaming_update(&mut self, active: bool, title: &str) {
        self.vencord.on_streaming_update(active, title);
        self.spotify.on_streaming_update(active, title);
        self.streaming.on_streaming_update(active, title);
        self.blyat.on_streaming_update(active, title);
        for plugin in &mut self.external {
            plugin.on_streaming_update(active, title);
        }
    }

    pub fn theme(&self) -> &AppTheme {
        self.vencord.theme()
    }

    pub fn spotify_activity(&self) -> Option<&Activity> {
        self.spotify.current()
    }

    pub fn spotify_progress_ms(&self) -> u64 {
        self.spotify.progress_ms()
    }

    pub fn spotify_duration_ms(&self) -> u64 {
        self.spotify.duration_ms()
    }

    pub fn streaming_state(&self) -> &StreamingState {
        self.streaming.state()
    }

    pub fn blyat_banner(&self) -> Option<&'static str> {
        self.blyat.banner_text()
    }

    pub fn names(&self) -> Vec<&str> {
        let mut names = vec![
            self.vencord.name(),
            self.spotify.name(),
            self.streaming.name(),
            self.blyat.name(),
        ];
        names.extend(self.external.iter().map(|plugin| plugin.name()));
        names
    }
}

fn init_one<P: Plugin>(plugin: &mut P, settings: &mut Settings) {
    let mut ctx = PluginContext { settings };
    if let Err(err) = plugin.init(&mut ctx) {
        log::warn!("plugin '{}' failed to initialize: {err:#}", plugin.name());
    }
}

/// Placeholder for the sandboxed WASM loader. The intended pipeline is:
///   1. read `plugins/<name>/plugin.json`
///   2. validate permissions (`read_messages`, `theme_injection`, ...)
///   3. instantiate the `.wasm` module with `wasmi` or `wasmtime`
///   4. wrap the instance in a `Plugin` adapter
///
/// The scaffold deliberately fails closed instead of pretending to sandbox
/// arbitrary code. Native plugins in this crate are registered explicitly.
pub fn load_wasm_plugin(_path: &std::path::Path) -> anyhow::Result<Box<dyn Plugin>> {
    anyhow::bail!(
        "WASM plugin loading is not wired up in this scaffold; register a native plugin instead"
    )
}
