use anyhow::{Context, Result};
use serde::Deserialize;

const SPOTIFY_API: &str = "https://api.spotify.com/v1";

pub struct SpotifyClient {
    http: reqwest::Client,
    access_token: String,
}

#[derive(Debug, Clone)]
pub struct SpotifyTrack {
    pub name: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_image: Option<String>,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub uri: String,
    /// `false` when Spotify reports the track loaded but paused. The strip keeps
    /// showing it with a pause marker instead of pretending nothing is there.
    pub is_playing: bool,
}

impl SpotifyClient {
    pub fn new(access_token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("Rustcord/0.1")
            .build()
            .context("failed to build Spotify HTTP client")?;
        Ok(Self {
            http,
            access_token: access_token.into(),
        })
    }

    pub async fn currently_playing(&self) -> Result<Option<SpotifyTrack>> {
        let response = self
            .http
            .get(format!("{SPOTIFY_API}/me/player/currently-playing"))
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("Spotify currently-playing request failed")?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        let response = response
            .error_for_status()
            .context("Spotify returned an error")?;

        let raw: CurrentlyPlaying = response
            .json()
            .await
            .context("could not decode Spotify response")?;

        Ok(raw.into_track())
    }
}

#[derive(Debug, Deserialize)]
struct CurrentlyPlaying {
    is_playing: bool,
    progress_ms: Option<u64>,
    item: Option<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    name: String,
    uri: String,
    duration_ms: u64,
    artists: Vec<Artist>,
    album: Album,
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Album {
    name: String,
    images: Vec<Image>,
}

#[derive(Debug, Deserialize)]
struct Image {
    url: String,
    width: u32,
    height: u32,
}

impl CurrentlyPlaying {
    fn into_track(self) -> Option<SpotifyTrack> {
        // A paused track is still a track: keep it so the strip can show where
        // playback stopped. Only "nothing loaded" means there is nothing to show.
        let item = self.item?;
        let album_image = item
            .album
            .images
            .into_iter()
            .max_by_key(|image| image.width * image.height)
            .map(|image| image.url);

        Some(SpotifyTrack {
            name: item.name,
            artists: item.artists.into_iter().map(|artist| artist.name).collect(),
            album: item.album.name,
            album_image,
            progress_ms: self.progress_ms.unwrap_or(0).min(item.duration_ms),
            duration_ms: item.duration_ms,
            uri: item.uri,
            is_playing: self.is_playing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(is_playing: bool) -> CurrentlyPlaying {
        serde_json::from_value(serde_json::json!({
            "is_playing": is_playing,
            "progress_ms": 61_000,
            "item": {
                "name": "Smooth Operator",
                "uri": "spotify:track:4hK9y0m6X6g0bXJ0Wn8m7K",
                "duration_ms": 270_000,
                "artists": [{ "name": "Sade" }],
                "album": {
                    "name": "Diamond Life",
                    "images": [
                        { "url": "https://i.scdn.co/image/small", "width": 64, "height": 64 },
                        { "url": "https://i.scdn.co/image/large", "width": 640, "height": 640 }
                    ]
                }
            }
        }))
        .expect("payload")
    }

    #[test]
    fn a_paused_track_is_kept_and_flagged() {
        let track = payload(false).into_track().expect("paused track");
        assert!(!track.is_playing);
        assert_eq!(track.name, "Smooth Operator");
        assert_eq!(
            track.album_image.as_deref(),
            Some("https://i.scdn.co/image/large")
        );
        assert_eq!(track.progress_ms, 61_000);

        let playing = payload(true).into_track().expect("playing track");
        assert!(playing.is_playing);
    }

    #[test]
    fn no_item_means_nothing_to_show() {
        let idle: CurrentlyPlaying = serde_json::from_value(serde_json::json!({
            "is_playing": false,
            "item": null
        }))
        .expect("payload");
        assert!(idle.into_track().is_none());
    }
}
