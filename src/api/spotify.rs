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
}

impl SpotifyClient {
    pub fn new(access_token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("Rustcord/0.1 (https://example.invalid)")
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
        if !self.is_playing {
            return None;
        }

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
        })
    }
}
