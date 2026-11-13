//! Bounded, asynchronous image loading for attachments and embeds.
//!
//! Three rules keep image-heavy channels inside the memory budget:
//!
//! 1. **Byte-bounded LRU.** Every texture is accounted at `width × height × 4`.
//!    When the budget is exceeded the least-recently-used texture is dropped,
//!    which also releases the GPU texture.
//! 2. **Only visible rows are requested.** The message list is virtualised, so
//!    scrolling past an image never downloads it.
//! 3. **Refuse surprises.** Only `https://` URLs on Discord's CDN hosts are
//!    fetched, downloads are capped, and a failed URL is remembered so a broken
//!    embed cannot cause a retry storm.
//!
//! Decoding happens on a dedicated worker thread; the UI only uploads finished
//! RGBA buffers, and never blocks on the network.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

pub const MAX_DOWNLOAD_BYTES: u64 = 12 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 4096;
/// GIFs are stopped at this many frames / decoded RGBA bytes. Guild icons are
/// 96 px, so these caps are generous for them while keeping a hostile upload
/// from allocating without bound.
const MAX_ANIMATION_FRAMES: usize = 64;
const MAX_ANIMATION_BYTES: usize = 64 * 1024 * 1024;

/// Discord's six numbered default avatars, embedded so a missing or still-loading
/// custom avatar can draw the real fallback on the first frame instead of an
/// initial. Same artwork the CDN serves at `embed/avatars/{n}.png`.
const DEFAULT_AVATAR_PNGS: [&[u8]; 6] = [
    include_bytes!("../assets/default-avatars/0.png"),
    include_bytes!("../assets/default-avatars/1.png"),
    include_bytes!("../assets/default-avatars/2.png"),
    include_bytes!("../assets/default-avatars/3.png"),
    include_bytes!("../assets/default-avatars/4.png"),
    include_bytes!("../assets/default-avatars/5.png"),
];

/// Hosts Rustcord will download images from. A crafted embed cannot make the
/// client fetch arbitrary URLs.
const ALLOWED_HOSTS: &[&str] = &[
    "cdn.discordapp.com",
    "i.scdn.co",
    "media.discordapp.net",
    "images-ext-1.discordapp.net",
    "images-ext-2.discordapp.net",
];

/// Whether a URL is safe to fetch.
pub fn is_allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };

    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();

    // Strip userinfo (`user@host`) and port before comparing the host.
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority);

    if host.is_empty() {
        return false;
    }

    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// Byte-budgeted LRU. Pure data structure so eviction is unit-testable without
/// a graphics context.
#[derive(Debug)]
pub struct ByteCache<K> {
    budget: usize,
    used: usize,
    clock: u64,
    entries: HashMap<K, CacheEntry>,
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    bytes: usize,
    last_used: u64,
}

impl<K: Eq + Hash + Clone> ByteCache<K> {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            clock: 0,
            entries: HashMap::new(),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Mark an entry as recently used.
    pub fn touch(&mut self, key: &K) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = clock;
        }
    }

    /// Insert (or replace) an entry, evicting until the budget holds.
    ///
    /// Returns the keys that were evicted. A single entry larger than the whole
    /// budget is evicted immediately, which keeps `used <= budget` absolute.
    pub fn insert(&mut self, key: K, bytes: usize) -> Vec<K> {
        if let Some(previous) = self.entries.remove(&key) {
            self.used = self.used.saturating_sub(previous.bytes);
        }

        self.clock += 1;
        self.entries.insert(
            key,
            CacheEntry {
                bytes,
                last_used: self.clock,
            },
        );
        self.used += bytes;

        self.evict_to_budget()
    }

    pub fn remove(&mut self, key: &K) -> Option<usize> {
        let entry = self.entries.remove(key)?;
        self.used = self.used.saturating_sub(entry.bytes);
        Some(entry.bytes)
    }

    /// Change the budget and evict the excess immediately.
    pub fn set_budget(&mut self, budget: usize) -> Vec<K> {
        self.budget = budget;
        self.evict_to_budget()
    }

    fn evict_to_budget(&mut self) -> Vec<K> {
        let mut evicted = Vec::new();
        while self.used > self.budget && !self.entries.is_empty() {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.used = self.used.saturating_sub(entry.bytes);
                evicted.push(oldest);
            }
        }
        evicted
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.used = 0;
    }
}

struct Job {
    url: String,
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<DecodedFrame>,
}

pub struct DecodedFrame {
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
}

struct Loaded {
    url: String,
    result: Result<Decoded, String>,
}

/// UI-side handle to the cache and the decode worker.
pub struct ImageLoader {
    cache: ByteCache<String>,
    textures: HashMap<String, egui::TextureHandle>,
    animations: HashMap<String, AnimatedTexture>,
    pending: HashSet<String>,
    failed: HashMap<String, String>,
    /// Created on the first image request, not at startup. A text-only session
    /// therefore keeps the two threads it always had, and never pays for the
    /// decode thread or reqwest's internal runtime.
    worker: Option<(Sender<Job>, Receiver<Loaded>)>,
    uploads: u64,
    rejected: u64,
    /// Uploaded once at startup from the embedded PNGs, so the fallback is
    /// available before the first frame and independent of the network.
    defaults: [Option<egui::TextureHandle>; 6],
}

impl ImageLoader {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            cache: ByteCache::new(budget_bytes),
            textures: HashMap::new(),
            animations: HashMap::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            worker: None,
            uploads: 0,
            rejected: 0,
            defaults: std::array::from_fn(|_| None),
        }
    }

    /// Decode and upload the six bundled default avatars. Idempotent; called once
    /// when the app is created, not per frame.
    pub fn load_default_avatars(&mut self, ctx: &egui::Context) {
        if self.defaults.iter().all(Option::is_some) {
            return;
        }
        for (index, bytes) in DEFAULT_AVATAR_PNGS.iter().enumerate() {
            if self.defaults[index].is_some() {
                continue;
            }
            let Ok(image) = image::load_from_memory(bytes) else {
                continue;
            };
            let image = image.to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            self.defaults[index] = Some(ctx.load_texture(
                format!("default-avatar:{index}"),
                color,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    pub fn default_avatar(&self, index: usize) -> Option<&egui::TextureHandle> {
        self.defaults.get(index)?.as_ref()
    }

    pub fn budget(&self) -> usize {
        self.cache.budget()
    }

    pub fn used(&self) -> usize {
        self.cache.used()
    }

    pub fn set_budget(&mut self, bytes: usize) {
        for key in self.cache.set_budget(bytes) {
            self.textures.remove(&key);
        }
    }

    /// `(bytes used, budget, textures held, downloads in flight)`
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        (
            self.cache.used(),
            self.cache.budget(),
            self.textures.len(),
            self.pending.len(),
        )
    }

    pub fn uploads(&self) -> u64 {
        self.uploads
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// Queue every URL that is not already cached, in flight, or known-bad.
    /// Returns how many new downloads were started.
    pub fn prepare(&mut self, urls: &[String]) -> usize {
        let mut started = 0;
        for url in urls {
            if self.textures.contains_key(url) {
                self.cache.touch(url);
                continue;
            }
            if self.pending.contains(url) || self.failed.contains_key(url) {
                continue;
            }
            if !is_allowed_url(url) {
                self.failed
                    .insert(url.clone(), "unsupported image URL".to_string());
                self.rejected += 1;
                continue;
            }

            if self.worker.is_none() {
                self.worker = Some(start_worker());
            }
            let Some((jobs, _)) = &self.worker else { break };
            if jobs.send(Job { url: url.clone() }).is_ok() {
                self.pending.insert(url.clone());
                started += 1;
            }
        }
        started
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The static texture, or the first frame of an animation. Callers that
    /// draw motion use [`Self::frame`] instead.
    pub fn peek(&self, url: &str) -> Option<&egui::TextureHandle> {
        self.frame(url, 0.0)
    }

    /// The texture for `url` at `time` seconds. Animations loop on their own GIF
    /// delays; static images ignore the clock.
    pub fn frame(&self, url: &str, time: f64) -> Option<&egui::TextureHandle> {
        if let Some(animation) = self.animations.get(url) {
            return animation.frame(time);
        }
        self.textures.get(url)
    }

    pub fn is_animated(&self, url: &str) -> bool {
        self.animations.contains_key(url)
    }

    pub fn is_pending(&self, url: &str) -> bool {
        self.pending.contains(url)
    }

    pub fn failure(&self, url: &str) -> Option<&str> {
        self.failed.get(url).map(String::as_str)
    }

    /// Upload finished downloads. Returns how many textures became available.
    pub fn poll(&mut self, ctx: &egui::Context) -> usize {
        let mut uploaded = 0;
        let mut disconnected = false;

        while let Some((_, results)) = &self.worker {
            let loaded = match results.try_recv() {
                Ok(loaded) => loaded,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            };

            self.pending.remove(&loaded.url);
            match loaded.result {
                Ok(decoded) => {
                    let size = [decoded.width as usize, decoded.height as usize];
                    let mut bytes = 0usize;
                    let mut frames = Vec::with_capacity(decoded.frames.len());
                    let mut delays = Vec::with_capacity(decoded.frames.len());
                    for (index, frame) in decoded.frames.into_iter().enumerate() {
                        bytes += frame.rgba.len();
                        delays.push(frame.delay_ms);
                        frames.push(ctx.load_texture(
                            format!("img:{}:{}", loaded.url, index),
                            egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba),
                            egui::TextureOptions::LINEAR,
                        ));
                    }

                    for evicted in self.cache.insert(loaded.url.clone(), bytes) {
                        self.textures.remove(&evicted);
                        self.animations.remove(&evicted);
                    }
                    if frames.len() > 1 {
                        self.animations
                            .insert(loaded.url, AnimatedTexture::new(frames, delays));
                    } else if let Some(texture) = frames.pop() {
                        self.textures.insert(loaded.url, texture);
                    }
                    self.uploads += 1;
                    uploaded += 1;
                }
                Err(err) => {
                    self.failed.insert(loaded.url, err);
                }
            }
        }

        if disconnected {
            self.worker = None;
        }

        uploaded
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        self.animations.clear();
        self.cache.clear();
        self.failed.clear();
    }
}

/// A decoded GIF held as one texture per frame. Total GPU bytes are charged to
/// the same byte cache as static images, so an animation cannot bypass the cap.
struct AnimatedTexture {
    frames: Vec<egui::TextureHandle>,
    delays_ms: Vec<u32>,
    total_ms: u64,
}

impl AnimatedTexture {
    fn new(frames: Vec<egui::TextureHandle>, delays_ms: Vec<u32>) -> Self {
        let delays_ms: Vec<u32> = delays_ms
            .into_iter()
            .map(|delay| delay.clamp(20, 10_000))
            .collect();
        let total_ms = delays_ms.iter().map(|delay| u64::from(*delay)).sum();
        Self {
            frames,
            delays_ms,
            total_ms,
        }
    }

    fn frame(&self, time: f64) -> Option<&egui::TextureHandle> {
        let elapsed = (time.max(0.0) * 1000.0) as u64;
        let index = frame_index(&self.delays_ms, self.total_ms, elapsed);
        self.frames.get(index).or_else(|| self.frames.first())
    }
}

/// Which frame is showing after `elapsed_ms`. GIF delays are per-frame, not a
/// fixed frame rate, and the animation wraps at the total duration.
fn frame_index(delays_ms: &[u32], total_ms: u64, elapsed_ms: u64) -> usize {
    if delays_ms.is_empty() || total_ms == 0 {
        return 0;
    }
    let mut remaining = elapsed_ms % total_ms;
    for (index, delay) in delays_ms.iter().enumerate() {
        let delay = u64::from(*delay);
        if remaining < delay {
            return index;
        }
        remaining -= delay;
    }
    delays_ms.len() - 1
}

/// Spawn the decode thread and return its two channel ends.
fn start_worker() -> (Sender<Job>, Receiver<Loaded>) {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (results_tx, results_rx) = mpsc::channel::<Loaded>();
    spawn_worker(jobs_rx, results_tx);
    (jobs_tx, results_rx)
}

fn spawn_worker(jobs: Receiver<Job>, results: Sender<Loaded>) {
    let _ = std::thread::Builder::new()
        .name("rustcord-images".to_string())
        .spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .user_agent("Rustcord/0.1")
                .timeout(std::time::Duration::from_secs(20))
                .build()
            {
                Ok(client) => client,
                Err(_) => return,
            };

            while let Ok(job) = jobs.recv() {
                let result = download(&client, &job.url);
                if results
                    .send(Loaded {
                        url: job.url,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
}

fn download(client: &reqwest::blocking::Client, url: &str) -> Result<Decoded, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("download failed: {err}"))?
        .error_for_status()
        .map_err(|err| format!("server refused: {err}"))?;

    if let Some(length) = response.content_length() {
        if length > MAX_DOWNLOAD_BYTES {
            return Err(format!("image too large ({length} bytes)"));
        }
    }

    let bytes = response
        .bytes()
        .map_err(|err| format!("could not read image: {err}"))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("image exceeded the download cap".to_string());
    }

    if is_animated_icon_url(url) {
        decode_gif(&bytes)
    } else {
        let image =
            image::load_from_memory(&bytes).map_err(|err| format!("decode failed: {err}"))?;
        check_dimensions(image.width(), image.height())?;
        let rgba = image.to_rgba8();
        Ok(Decoded {
            width: rgba.width(),
            height: rgba.height(),
            frames: vec![DecodedFrame {
                rgba: rgba.into_raw(),
                delay_ms: 0,
            }],
        })
    }
}

fn is_gif_url(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase()
        .ends_with(".gif")
}

/// Only guild icons are animated. Other GIFs (attachments, embeds) keep their
/// existing first-frame treatment so a large animation cannot fill the texture
/// cache with frames the UI never draws.
fn is_animated_icon_url(url: &str) -> bool {
    is_gif_url(url) && url.contains("/icons/")
}

fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!(
            "image is {width}×{height}, above the {MAX_DIMENSION} px limit"
        ));
    }
    Ok(())
}

fn decode_gif(bytes: &[u8]) -> Result<Decoded, String> {
    use image::AnimationDecoder;

    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("decode failed: {err}"))?;
    let mut decoded = Vec::new();
    let mut bytes_used = 0usize;
    let mut size: Option<(u32, u32)> = None;
    for frame in decoder.into_frames().take(MAX_ANIMATION_FRAMES) {
        let frame = frame.map_err(|err| format!("decode failed: {err}"))?;
        let delay: std::time::Duration = frame.delay().into();
        let rgba = frame.into_buffer();
        check_dimensions(rgba.width(), rgba.height())?;
        match size {
            None => size = Some((rgba.width(), rgba.height())),
            Some(expected) if expected != (rgba.width(), rgba.height()) => {
                return Err("animation frames changed size".to_string());
            }
            Some(_) => {}
        }
        bytes_used = bytes_used.saturating_add(rgba.as_raw().len());
        if bytes_used > MAX_ANIMATION_BYTES {
            break;
        }
        decoded.push(DecodedFrame {
            rgba: rgba.into_raw(),
            delay_ms: u32::try_from(delay.as_millis()).unwrap_or(u32::MAX),
        });
    }

    let Some((width, height)) = size else {
        return Err("animation had no frames".to_string());
    };
    Ok(Decoded {
        width,
        height,
        frames: decoded,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_https_discord_cdn_hosts() {
        assert!(is_allowed_url(
            "https://cdn.discordapp.com/attachments/1/2/cat.png"
        ));
        assert!(is_allowed_url(
            "https://media.discordapp.net/attachments/1/2/cat.png?width=400"
        ));
        assert!(is_allowed_url("https://cdn.discordapp.com:443/x.png"));
        assert!(is_allowed_url("https://i.scdn.co/image/large"));
        assert!(!is_allowed_url("http://i.scdn.co/image/large"));
        assert!(!is_allowed_url("https://i.scdn.co.evil.test/image"));
    }

    #[test]
    fn rejects_everything_else() {
        assert!(!is_allowed_url("http://cdn.discordapp.com/x.png"));
        assert!(!is_allowed_url("https://example.com/x.png"));
        assert!(!is_allowed_url("https://cdn.discordapp.com.evil.io/x.png"));
        assert!(!is_allowed_url("https://cdn.discordapp.com@evil.io/x.png"));
        assert!(!is_allowed_url("file:///etc/passwd"));
        assert!(!is_allowed_url("data:image/png;base64,AAAA"));
        assert!(!is_allowed_url("https:///x.png"));
    }

    #[test]
    fn cache_stays_within_budget() {
        let mut cache = ByteCache::new(100);
        assert!(cache.insert("a".to_string(), 60).is_empty());
        let evicted = cache.insert("b".to_string(), 60);
        assert_eq!(evicted, vec!["a".to_string()]);
        assert!(cache.used() <= cache.budget());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn least_recently_used_is_evicted_first() {
        let mut cache = ByteCache::new(100);
        cache.insert("a".to_string(), 40);
        cache.insert("b".to_string(), 40);
        cache.touch(&"a".to_string()); // a is now newer than b

        let evicted = cache.insert("c".to_string(), 40);
        assert_eq!(evicted, vec!["b".to_string()]);
        assert!(cache.contains(&"a".to_string()));
        assert!(cache.contains(&"c".to_string()));
    }

    #[test]
    fn oversized_single_image_is_evicted_immediately() {
        let mut cache = ByteCache::new(50);
        let evicted = cache.insert("huge".to_string(), 500);
        assert_eq!(evicted, vec!["huge".to_string()]);
        assert!(cache.is_empty());
        assert_eq!(cache.used(), 0);
    }

    #[test]
    fn replacing_an_entry_updates_accounting() {
        let mut cache = ByteCache::new(100);
        cache.insert("a".to_string(), 80);
        cache.insert("a".to_string(), 10);
        assert_eq!(cache.used(), 10);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn shrinking_the_budget_evicts_down_to_it() {
        let mut cache = ByteCache::new(300);
        for key in ["a", "b", "c"] {
            cache.insert(key.to_string(), 100);
        }
        let evicted = cache.set_budget(150);
        assert_eq!(evicted.len(), 2);
        assert!(cache.used() <= 150);
    }

    #[test]
    fn clearing_releases_the_accounting() {
        let mut cache = ByteCache::new(100);
        cache.insert("a".to_string(), 90);
        cache.clear();
        assert_eq!(cache.used(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn bundled_default_avatars_decode() {
        assert_eq!(DEFAULT_AVATAR_PNGS.len(), 6);
        for (index, bytes) in DEFAULT_AVATAR_PNGS.iter().enumerate() {
            let image = image::load_from_memory(bytes)
                .unwrap_or_else(|err| panic!("default avatar {index}: {err}"))
                .to_rgba8();
            assert_eq!(image.dimensions(), (256, 256), "avatar {index}");
        }
    }

    /// The decode path, without the network: a 2×2 PNG costs exactly 16 bytes
    /// of RGBA, which is what the cache must account for.
    #[test]
    fn decodes_png_and_accounts_for_rgba_bytes() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("png header");
            writer.write_image_data(&[255; 16]).expect("png data");
        }

        let decoded = image::load_from_memory(&png).expect("decode").to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert_eq!(decoded.into_raw().len(), 16);

        let mut cache = ByteCache::new(1024);
        cache.insert("2x2".to_string(), 16);
        assert_eq!(cache.used(), 16);
    }

    /// 40 screenshots at 2560×1440 would be 590 MB decoded; the cache must hold
    /// the configured budget no matter how many arrive.
    #[test]
    fn a_channel_full_of_screenshots_cannot_exceed_the_budget() {
        let budget = 48 * 1024 * 1024;
        let screenshot = 2560usize * 1440 * 4; // 14.7 MB per image
        let mut cache = ByteCache::new(budget);

        let mut evicted = 0;
        for index in 0..40 {
            evicted += cache.insert(format!("shot-{index}"), screenshot).len();
        }

        assert!(
            cache.used() <= budget,
            "used {} > budget {budget}",
            cache.used()
        );
        assert_eq!(cache.len(), budget / screenshot);
        assert!(evicted >= 37, "expected continuous eviction, got {evicted}");
    }

    #[test]
    fn prepare_rejects_bad_urls_and_dedupes_requests() {
        let mut loader = ImageLoader::new(1024);
        let rejected = vec!["http://evil/x.png".to_string()];
        assert_eq!(loader.prepare(&rejected), 0);
        assert_eq!(loader.rejected(), 1);
        assert!(loader.failure("http://evil/x.png").is_some());
        // A rejected URL is never retried.
        assert_eq!(loader.prepare(&rejected), 0);
        assert_eq!(loader.rejected(), 1);
    }

    #[test]
    fn gif_frames_advance_by_their_own_delays_and_loop() {
        let delays = [100, 50, 250];
        let total: u64 = delays.iter().map(|delay| u64::from(*delay)).sum();
        assert_eq!(frame_index(&delays, total, 0), 0);
        assert_eq!(frame_index(&delays, total, 99), 0);
        assert_eq!(frame_index(&delays, total, 100), 1);
        assert_eq!(frame_index(&delays, total, 149), 1);
        assert_eq!(frame_index(&delays, total, 150), 2);
        assert_eq!(frame_index(&delays, total, 399), 2);
        // 400 ms is the loop point again.
        assert_eq!(frame_index(&delays, total, 400), 0);
        assert_eq!(frame_index(&delays, total, 1_001), 2);
    }

    #[test]
    fn gif_decoding_keeps_every_frame_and_its_delay() {
        use image::codecs::gif::{GifEncoder, Repeat};
        use image::{Delay, Frame, RgbaImage};

        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).expect("repeat");
            for (color, delay) in [([255, 0, 0, 255], 100), ([0, 0, 255, 255], 40)] {
                let buffer = RgbaImage::from_pixel(3, 2, image::Rgba(color));
                encoder
                    .encode_frame(Frame::from_parts(
                        buffer,
                        0,
                        0,
                        Delay::from_numer_denom_ms(delay, 1),
                    ))
                    .expect("frame");
            }
        }

        let decoded = decode_gif(&bytes).expect("decode");
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].delay_ms, 100);
        assert_eq!(decoded.frames[1].delay_ms, 40);
        assert_eq!(decoded.frames[0].rgba[..4], [255, 0, 0, 255]);
        assert_eq!(decoded.frames[1].rgba[..4], [0, 0, 255, 255]);
    }

    #[test]
    fn gif_urls_are_recognised_before_the_query_string() {
        assert!(is_gif_url(
            "https://cdn.discordapp.com/icons/1/a_x.gif?size=96"
        ));
        assert!(is_gif_url("https://cdn.discordapp.com/icons/1/a_x.GIF"));
        assert!(!is_gif_url(
            "https://cdn.discordapp.com/icons/1/x.png?format=gif"
        ));
    }
}
