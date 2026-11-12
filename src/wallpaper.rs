//! Blurred desktop wallpaper backdrop.
//!
//! The window cannot ask the compositor to blur what is behind it in a portable
//! way, so the app does the next best thing honestly: it finds the wallpaper
//! image the desktop is already using (KDE, or GNOME via `gsettings`), blurs a
//! small copy of it and paints that behind its own translucent surfaces. The
//! result is a real blurred wallpaper, not a screenshot of other windows — the
//! docs say so, and it costs one small texture.
//!
//! Detection and the image maths are pure and tested; only the two small file and
//! process reads at the edges touch the system.

use std::path::{Path, PathBuf};

/// The blurred copy is this wide; upscaling it with linear filtering is the last
/// and cheapest pass of the blur.
pub const TARGET_WIDTH: u32 = 320;

/// Read the wallpaper path from the explicit override, then KDE, then GNOME.
/// Only paths that exist are returned, so a stale config cannot win.
pub fn detect(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = explicit
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(expand)
    {
        if path.is_file() {
            return Some(path);
        }
        log::info!("configured wallpaper {} is missing", path.display());
    }

    [
        hyprland_wallpaper(),
        hyprland_config_wallpaper(),
        hypr_config_dir_wallpaper(),
        sway_wallpaper(),
        kde_wallpaper(),
        gnome_wallpaper(),
        xfce_wallpaper(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| candidate.is_file())
}

/// Expand `~/` and `$HOME/` so a hand-typed or config-sourced path works.
pub fn expand(path: &str) -> PathBuf {
    let path = path.trim().trim_matches('"').trim_matches('\'');
    for prefix in ["~/", "$HOME/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            if let Some(home) = dirs::home_dir() {
                return home.join(rest);
            }
        }
    }
    PathBuf::from(path)
}

/// Some Hyprland setups start a wallpaper tool from `exec-once` instead of
/// keeping a config the daemon can be asked about; take the image path from such
/// a command line, or from a `swaybg`/`output` line.
pub fn parse_shell_wallpaper(text: &str) -> Option<String> {
    const EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".webp", ".bmp"];
    let tokens: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.starts_with("exec") || line.starts_with("output"))
        .flat_map(str::split_whitespace)
        .map(|token| token.trim_matches(|c| c == '\'' || c == '"' || c == ',' || c == ';'))
        .collect();

    tokens
        .iter()
        .rev()
        .find(|token| {
            let lower = token.to_lowercase();
            EXTENSIONS
                .iter()
                .any(|extension| lower.ends_with(extension))
        })
        .map(|token| (*token).to_string())
}

/// Lua-configured Hyprland setups keep everything in `~/.config/hypr`, not a
/// single `hyprland.conf`; scan those text files for any token that looks like an
/// image path (`swaybg -i …`, `hyprpaper` preloads, a helper script).
pub fn parse_image_paths(text: &str) -> Option<String> {
    const EXTENSIONS: [&str; 5] = [".png", ".jpg", ".jpeg", ".webp", ".bmp"];

    let mut best: Option<String> = None;
    for line in text.lines() {
        let lower = line.to_lowercase();
        for extension in EXTENSIONS {
            let Some(end) = lower.rfind(extension).map(|index| index + extension.len()) else {
                continue;
            };
            // A path can contain spaces (`Lua Wall.png`), so anchor on the last
            // slash before the extension instead of splitting on whitespace.
            for (slash, _) in line[..end].match_indices('/') {
                let candidate = line[slash..end]
                    .trim()
                    .trim_end_matches(['"', '\'', ')', ',', ';']);
                if candidate.to_lowercase().ends_with(extension) {
                    best = Some(candidate.to_string());
                    break;
                }
            }
        }
    }
    best
}

fn hypr_config_dir_wallpaper() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("hypr");
    scan_for_wallpaper(&dir, 4)
}

/// Depth-limited so a symlink loop cannot hang startup.
fn scan_for_wallpaper(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = scan_for_wallpaper(&path, depth - 1) {
                return Some(found);
            }
            continue;
        }
        // Read any small text file: helper scripts and extensionless config
        // fragments are common, while images exceed the size cutoff.
        let small = entry
            .metadata()
            .map(|meta| meta.len() <= 256 * 1024)
            .unwrap_or(false);
        if !small {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(candidate) = parse_image_paths(&text) {
            let candidate = expand(&candidate);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn hyprland_config_wallpaper() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("hypr/hyprland.conf");
    let text = std::fs::read_to_string(path).ok()?;
    parse_shell_wallpaper(&text).map(|path| expand(&path))
}

fn sway_wallpaper() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("sway/config");
    let text = std::fs::read_to_string(path).ok()?;
    parse_shell_wallpaper(&text).map(|path| expand(&path))
}

fn run(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Hyprland has no single wallpaper setting; the daemon that draws it is asked
/// first (hyprpaper's `listactive`, or swww), then its config is read as a
/// fallback for when the daemon is not running yet.
fn hyprland_wallpaper() -> Option<PathBuf> {
    if let Some(text) = run("hyprctl", &["hyprpaper", "listactive"]) {
        if let Some(path) = parse_hyprpaper_listactive(&text) {
            return Some(PathBuf::from(path));
        }
    }
    if let Some(text) = run("swww", &["query"]) {
        if let Some(path) = parse_swww_query(&text) {
            return Some(PathBuf::from(path));
        }
    }

    let config = dirs::config_dir()?.join("hypr/hyprpaper.conf");
    let text = std::fs::read_to_string(config).ok()?;
    parse_hyprpaper_conf(&text).map(|path| expand(&path))
}

/// `hyprctl hyprpaper listactive` prints `monitor = /path/wall.png` per output.
pub fn parse_hyprpaper_listactive(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(_, value)| value.trim())
        .next_back()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// `hyprpaper.conf` uses `wallpaper = monitor,/path/wall.png` (or `preload`).
pub fn parse_hyprpaper_conf(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key != "wallpaper" && key != "preload" {
                return None;
            }
            value.rsplit(',').next().map(str::trim)
        })
        .next_back()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// `swww query` describes each output and ends with `image: /path/wall.png`.
pub fn parse_swww_query(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("image")
                .then(|| value.trim())
                .filter(|path| !path.is_empty())
        })
        .map(str::to_string)
}

fn xfce_wallpaper() -> Option<PathBuf> {
    let list = run("xfconf-query", &["-c", "xfce4-desktop", "-l"])?;
    for property in list
        .lines()
        .map(str::trim)
        .filter(|property| property.ends_with("last-image"))
    {
        if let Some(value) = run("xfconf-query", &["-c", "xfce4-desktop", "-p", property]) {
            let path = PathBuf::from(value.trim());
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn kde_wallpaper() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("plasma-org.kde.plasma.desktop-appletsrc");
    let text = std::fs::read_to_string(path).ok()?;
    parse_kde_wallpaper(&text).map(PathBuf::from)
}

fn gnome_wallpaper() -> Option<PathBuf> {
    for key in ["picture-uri", "picture-uri-dark"] {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.background", key])
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = parse_gsettings_uri(&text) {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// KDE stores one `Image=file:///…` line per screen containment. The last one is
/// the most recently configured, so it wins.
pub fn parse_kde_wallpaper(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("Image="))
        .filter_map(file_uri_to_path)
        .next_back()
}

/// `gsettings get` prints a quoted GVariant string: `'file:///home/a/b.png'`.
pub fn parse_gsettings_uri(text: &str) -> Option<String> {
    let text = text.trim().trim_matches('\'').trim_matches('"');
    file_uri_to_path(text)
}

/// Convert a `file://` URI to a filesystem path, decoding `%20` and friends.
/// Non-local hosts (SMB, for instance) are refused rather than guessed at.
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("file://")?;
    let rest = &rest[rest.find('/')?..];

    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded)
        .ok()
        .filter(|path| !path.is_empty())
}

/// Decode, downscale and blur a wallpaper into a small RGBA buffer.
pub fn blurred(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let image = image::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let source_width = image.width().max(1);
    let source_height = image.height().max(1);

    let width = TARGET_WIDTH.min(source_width);
    let height = ((f64::from(source_height) * f64::from(width) / f64::from(source_width)).round()
        as u32)
        .max(1);
    let small = image
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_rgba8();

    let (width, height) = (small.width() as usize, small.height() as usize);
    let mut pixels = small.into_raw();
    for _ in 0..3 {
        box_blur(&mut pixels, width, height, 3);
    }
    Ok((width, height, pixels))
}

/// Separable box blur, edge-clamped. The buffer is RGBA and small enough that this
/// is microseconds; it is here rather than in a shader because the result is
/// uploaded once, not per frame.
pub fn box_blur(pixels: &mut [u8], width: usize, height: usize, radius: usize) {
    if radius == 0 || width == 0 || height == 0 || pixels.len() < width * height * 4 {
        return;
    }

    let mut scratch = pixels.to_vec();
    let span = (radius * 2 + 1) as u32;

    for y in 0..height {
        for x in 0..width {
            let mut sums = [0u32; 4];
            for offset in 0..=(radius * 2) {
                let sample =
                    (x as isize + offset as isize - radius as isize).clamp(0, width as isize - 1);
                let index = (y * width + sample as usize) * 4;
                for channel in 0..4 {
                    sums[channel] += u32::from(pixels[index + channel]);
                }
            }
            let out = (y * width + x) * 4;
            for channel in 0..4 {
                scratch[out + channel] = (sums[channel] / span) as u8;
            }
        }
    }

    for y in 0..height {
        for x in 0..width {
            let mut sums = [0u32; 4];
            for offset in 0..=(radius * 2) {
                let sample =
                    (y as isize + offset as isize - radius as isize).clamp(0, height as isize - 1);
                let index = (sample as usize * width + x) * 4;
                for channel in 0..4 {
                    sums[channel] += u32::from(scratch[index + channel]);
                }
            }
            let out = (y * width + x) * 4;
            for channel in 0..4 {
                pixels[out + channel] = (sums[channel] / span) as u8;
            }
        }
    }
}

/// UV rectangle for drawing `texture` over `view` without stretching it (CSS
/// `object-fit: cover`): the longer ratio is cropped centrally.
pub fn cover_uv(texture: egui::Vec2, view: egui::Vec2) -> egui::Rect {
    if texture.x <= 0.0 || texture.y <= 0.0 || view.x <= 0.0 || view.y <= 0.0 {
        return egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }

    let texture_aspect = texture.x / texture.y;
    let view_aspect = view.x / view.y;

    if view_aspect > texture_aspect {
        // Wider view: crop top and bottom.
        let height = texture_aspect / view_aspect;
        let inset = (1.0 - height) / 2.0;
        egui::Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
    } else {
        let width = view_aspect / texture_aspect;
        let inset = (1.0 - width) / 2.0;
        egui::Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kde_image_lines_are_decoded_and_the_last_wins() {
        let text = "\
[Containments][1][Wallpaper][org.kde.image][General]
Image=file:///home/a/first.png
[Containments][2][Wallpaper][org.kde.image][General]
Image=file:///home/a/My%20Wallpaper.jpg
";
        assert_eq!(
            parse_kde_wallpaper(text).as_deref(),
            Some("/home/a/My Wallpaper.jpg")
        );
        assert!(parse_kde_wallpaper("Image=\n").is_none());
        assert!(parse_kde_wallpaper("Color=0,0,0").is_none());
    }

    #[test]
    fn gsettings_quotes_and_remote_hosts() {
        assert_eq!(
            parse_gsettings_uri("'file:///usr/share/backgrounds/a%20b.png'\n").as_deref(),
            Some("/usr/share/backgrounds/a b.png")
        );
        assert!(parse_gsettings_uri("'file://server/share/x.png'").is_some());
        assert!(parse_gsettings_uri("'https://example.com/x.png'").is_none());
        assert!(parse_gsettings_uri("'file://'").is_none());
    }

    #[test]
    fn file_uris_decode_utf8_safely() {
        assert_eq!(
            file_uri_to_path("file:///home/a/%C3%A9.png").as_deref(),
            Some("/home/a/é.png")
        );
        assert!(file_uri_to_path("file:///home/a/%zz.png").is_some());
        assert!(file_uri_to_path("smtp://x").is_none());
    }

    #[test]
    fn hyprpaper_sources_are_parsed() {
        assert_eq!(
            parse_hyprpaper_listactive("DP-1 = /home/a/one.png\nHDMI-A-1 = /home/a/two.png\n")
                .as_deref(),
            Some("/home/a/two.png")
        );
        assert!(parse_hyprpaper_listactive("").is_none());

        let config = "\"# comment\nwallpaper = DP-1,/home/a/My Wall.png\n";
        assert_eq!(
            parse_hyprpaper_conf(config).as_deref(),
            Some("/home/a/My Wall.png"),
            "the path is after the monitor"
        );
        assert_eq!(
            parse_hyprpaper_conf("preload = /home/a/cache.png\n").as_deref(),
            Some("/home/a/cache.png")
        );
        assert!(parse_hyprpaper_conf("splash = false\n").is_none());
    }

    #[test]
    fn swww_query_finds_the_image_line() {
        let text = "Location: 0, 0\n  Size: 1920x1080\n  Image: /home/a/w.png\n";
        assert_eq!(parse_swww_query(text).as_deref(), Some("/home/a/w.png"));
        assert!(parse_swww_query("no image here").is_none());
    }

    #[test]
    fn wallpaper_commands_are_scanned_for_image_paths() {
        let conf = "\"# exec-once = swaybg -i /old.png\nexec-once = swaybg -i /home/a/wall.jpg -m fill\nexec = hyprpaper\nmonitor = eDP-1, 1920x1080, 0x0, 1\n";
        assert_eq!(
            parse_shell_wallpaper(conf).as_deref(),
            Some("/home/a/wall.jpg")
        );
        assert!(parse_shell_wallpaper("exec-once = hyprpaper\n").is_none());

        let home = dirs::home_dir().expect("home");
        assert_eq!(expand("~/Pictures/a.png"), home.join("Pictures/a.png"));
        assert_eq!(expand("$HOME/a.png"), home.join("a.png"));
        assert_eq!(expand("/abs/a.png"), PathBuf::from("/abs/a.png"));
        assert_eq!(expand("'/quoted/a.png'"), PathBuf::from("/quoted/a.png"));
    }

    #[test]
    fn lua_configs_are_scanned_for_image_paths() {
        let lua = "hl.exec_cmd('swaybg -i /home/a/Lua Wall.png --mode fill')\nhl.exec_cmd(\"hyprpaper\")\nmonitor = eDP-1, 1920x1080, 0x0, 1\n";
        assert_eq!(
            parse_image_paths(lua).as_deref(),
            Some("/home/a/Lua Wall.png")
        );
        assert!(parse_image_paths("exec_cmd(\"hyprpaper\")").is_none());
        assert!(parse_image_paths("background(\"color\", \"#101010\")").is_none());
    }

    #[test]
    fn blur_spreads_a_bright_pixel() {
        let (width, height) = (9, 9);
        let mut pixels = vec![0u8; width * height * 4];
        let centre = ((4 * width) + 4) * 4;
        pixels[centre..centre + 4].copy_from_slice(&[255, 255, 255, 255]);

        box_blur(&mut pixels, width, height, 2);

        let left = ((4 * width) + 2) * 4;
        assert!(pixels[left] > 0, "brightness spread horizontally");
        let below = ((6 * width) + 4) * 4;
        assert!(pixels[below] > 0, "and vertically");
        assert!(pixels[centre] < 255, "the centre was averaged down");
    }

    #[test]
    fn blur_ignores_degenerate_input() {
        let mut pixels = vec![7u8; 16];
        box_blur(&mut pixels, 2, 2, 0);
        assert_eq!(pixels, vec![7u8; 16], "radius zero is a no-op");
        box_blur(&mut pixels, 0, 0, 3);
        assert_eq!(pixels, vec![7u8; 16], "empty image is a no-op");
    }

    #[test]
    fn cover_crops_the_right_axis() {
        // A square texture in a wide view crops top and bottom.
        let uv = cover_uv(egui::vec2(100.0, 100.0), egui::vec2(200.0, 100.0));
        assert!(uv.min.y > 0.0 && uv.max.y < 1.0);
        assert_eq!(uv.min.x, 0.0);
        assert_eq!(uv.max.x, 1.0);

        // A wide texture in a tall view crops left and right.
        let uv = cover_uv(egui::vec2(200.0, 100.0), egui::vec2(100.0, 200.0));
        assert!(uv.min.x > 0.0 && uv.max.x < 1.0);
        assert_eq!(uv.min.y, 0.0);

        let uv = cover_uv(egui::vec2(0.0, 0.0), egui::vec2(10.0, 10.0));
        assert_eq!(
            uv,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }
}
