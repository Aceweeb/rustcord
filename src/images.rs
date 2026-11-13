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
    pub rgba: Vec<u8>,
}

struct Loaded {
    url: String,
    result: Result<Decoded, String>,
}

/// UI-side handle to the cache and the decode worker.
pub struct ImageLoader {
    cache: ByteCache<String>,
    textures: HashMap<String, egui::TextureHandle>,
    pending: HashSet<String>,
    failed: HashMap<String, String>,
    /// Created on the first image request, not at startup. A text-only session
    /// therefore keeps the two threads it always had, and never pays for the
    /// decode thread or reqwest's internal runtime.
    worker: Option<(Sender<Job>, Receiver<Loaded>)>,
    uploads: u64,
    rejected: u64,
}

impl ImageLoader {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            cache: ByteCache::new(budget_bytes),
            textures: HashMap::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            worker: None,
            uploads: 0,
            rejected: 0,
        }
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

    pub fn peek(&self, url: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(url)
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
                    let bytes = decoded.rgba.len();
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [decoded.width as usize, decoded.height as usize],
                        &decoded.rgba,
                    );
                    let texture = ctx.load_texture(
                        format!("img:{}", loaded.url),
                        image,
                        egui::TextureOptions::LINEAR,
                    );

                    for evicted in self.cache.insert(loaded.url.clone(), bytes) {
                        self.textures.remove(&evicted);
                    }
                    self.textures.insert(loaded.url, texture);
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
        self.cache.clear();
        self.failed.clear();
    }
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

    let image = image::load_from_memory(&bytes).map_err(|err| format!("decode failed: {err}"))?;
    if image.width() > MAX_DIMENSION || image.height() > MAX_DIMENSION {
        return Err(format!(
            "image is {}×{}, above the {MAX_DIMENSION} px limit",
            image.width(),
            image.height()
        ));
    }

    let rgba = image.to_rgba8();
    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
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
}
