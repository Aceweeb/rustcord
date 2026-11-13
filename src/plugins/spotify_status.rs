use crate::models::{Activity, Message, Reaction};
use crate::plugins::{Plugin, PluginContext};
use crate::settings::Settings;

/// Spotify "now playing" integration. The network path lives in
/// `crate::api::spotify`; this plugin owns the presentation state and the
/// Discord Rich Presence payload.
pub struct SpotifyStatusPlugin {
    enabled: bool,
    current: Option<Activity>,
    progress_ms: u64,
    duration_ms: u64,
    last_tick: f32,
}

impl Default for SpotifyStatusPlugin {
    fn default() -> Self {
        Self {
            enabled: true,
            current: None,
            progress_ms: 0,
            duration_ms: 0,
            last_tick: 0.0,
        }
    }
}

impl SpotifyStatusPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_track(
        &mut self,
        track: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        uri: impl Into<String>,
        progress_ms: u64,
        duration_ms: u64,
    ) {
        let track = track.into();
        let artist = artist.into();
        let album = album.into();
        let uri = uri.into();

        self.current = Some(Activity {
            name: track,
            details: Some(artist),
            state: Some(album),
            large_image: None,
            url: Some(uri),
        });
        self.progress_ms = progress_ms.min(duration_ms);
        self.duration_ms = duration_ms;
    }

    /// Apply a track from the live poller, including its album art URL.
    pub fn set_from_track(&mut self, track: &crate::api::spotify::SpotifyTrack) {
        self.set_track(
            track.name.clone(),
            track.artists.join(", "),
            track.album.clone(),
            track.uri.clone(),
            track.progress_ms,
            track.duration_ms,
        );
        if let Some(current) = &mut self.current {
            current.large_image = track.album_image.clone();
        }
    }

    /// Nothing is playing: drop the state instead of keeping the last track.
    pub fn clear(&mut self) {
        self.current = None;
        self.progress_ms = 0;
        self.duration_ms = 0;
    }

    pub fn current(&self) -> Option<&Activity> {
        self.current.as_ref()
    }

    pub fn progress_ms(&self) -> u64 {
        self.progress_ms
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Advance the locally displayed progress. The real implementation should
    /// reconcile this with `GET /me/player` every few seconds.
    pub fn tick(&mut self, now: f32) {
        if !self.enabled || self.duration_ms == 0 {
            self.last_tick = now;
            return;
        }

        let dt = (now - self.last_tick).max(0.0) as f64;
        self.last_tick = now;
        self.progress_ms = self
            .progress_ms
            .saturating_add((dt * 1000.0) as u64)
            .min(self.duration_ms);
    }
}

impl Plugin for SpotifyStatusPlugin {
    fn name(&self) -> &str {
        "spotify-status"
    }

    fn init(&mut self, ctx: &mut PluginContext<'_>) -> anyhow::Result<()> {
        self.enabled = ctx.settings.spotify_status_sync;
        // Start empty: the now-playing strip belongs to real playback, so it must
        // not appear until the poller reports a track that is actually playing.
        self.clear();

        Ok(())
    }

    fn on_message(&mut self, _message: &mut Message) {}

    fn on_react(&mut self, _reaction: &mut Reaction) {}

    fn on_settings(&mut self, settings: &mut Settings) {
        self.enabled = settings.spotify_status_sync;
        if !self.enabled {
            self.current = None;
            self.progress_ms = 0;
            self.duration_ms = 0;
        }
    }

    fn on_activity_update(&mut self, activity: &mut Activity) {
        if self.enabled {
            if let Some(current) = &self.current {
                *activity = current.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_playing_until_spotify_reports_a_track() {
        let mut plugin = SpotifyStatusPlugin::new();
        let mut settings = Settings::default();
        plugin
            .init(&mut PluginContext {
                settings: &mut settings,
            })
            .expect("init");
        assert!(plugin.current().is_none());
        assert_eq!(plugin.duration_ms(), 0);
    }
}
