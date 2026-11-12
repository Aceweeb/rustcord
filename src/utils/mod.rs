//! Colour utilities used by the theme engine and the UI.
//!
//! Everything here is pure so it can be unit-tested without an egui context.

use egui::Color32;

/// Return the first grapheme-ish character of a display name, uppercased.
pub fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string())
}

/// Lighten (or darken, with a negative delta) a colour by a flat amount per
/// channel. Used for Discord's hover/selected rows, which are the surface colour
/// lifted a little rather than a separate palette entry.
pub fn lift(color: Color32, delta: i16) -> Color32 {
    let channel = |value: u8| (i16::from(value) + delta).clamp(0, 255) as u8;
    Color32::from_rgba_unmultiplied(
        channel(color.r()),
        channel(color.g()),
        channel(color.b()),
        color.a(),
    )
}

/// Blend two colours: `t == 0.0` is `a`, `t == 1.0` is `b`. Alpha follows `a`,
/// so a tint over a surface keeps the surface's opacity (D42).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel =
        |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        a.a(),
    )
}

/// Parse `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)` and `rgba(r,g,b,a)`.
pub fn parse_css_color(input: &str) -> Option<Color32> {
    let value = input.trim().trim_end_matches(';').trim();

    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            _ => None,
        };
    }

    if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<_> = inner.split(',').map(str::trim).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(Color32::from_rgb(r, g, b));
        }
    }

    if let Some(inner) = value
        .strip_prefix("rgba(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<_> = inner.split(',').map(str::trim).collect();
        if parts.len() == 4 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            let a = parts[3].parse::<f32>().ok()?;
            return Some(Color32::from_rgba_unmultiplied(
                r,
                g,
                b,
                (a.clamp(0.0, 1.0) * 255.0) as u8,
            ));
        }
    }

    None
}

/// `#rrggbb` representation, used for persistence and the settings hex field.
pub fn color_to_hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// WCAG relative luminance of an sRGB colour (0.0 = black, 1.0 = white).
pub fn relative_luminance(color: Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let c = value as f32 / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// WCAG contrast ratio between two colours, from 1.0 (identical) to 21.0.
/// Useful both for theme QA tests and for picking readable accent text.
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la > lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Pick black or white, whichever reads better on `background`.
pub fn readable_on(background: Color32) -> Color32 {
    let black = Color32::from_rgb(0, 0, 0);
    let white = Color32::from_rgb(255, 255, 255);
    if contrast_ratio(background, black) >= contrast_ratio(background, white) {
        black
    } else {
        white
    }
}

pub fn format_duration(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_moves_between_the_two_colours() {
        let dark = Color32::from_rgb(0, 0, 0);
        let light = Color32::from_rgb(200, 100, 50);
        assert_eq!(mix(dark, light, 0.0), dark);
        assert_eq!(mix(dark, light, 1.0), light);
        assert_eq!(mix(dark, light, 0.5), Color32::from_rgb(100, 50, 25));
        // Out-of-range weights clamp instead of wrapping.
        assert_eq!(mix(dark, light, -2.0), dark);
        assert_eq!(mix(dark, light, 3.0), light);
    }

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(
            parse_css_color("#fff"),
            Some(Color32::from_rgb(255, 255, 255))
        );
        assert_eq!(
            parse_css_color("#5865f2"),
            Some(Color32::from_rgb(0x58, 0x65, 0xf2))
        );
        assert_eq!(
            parse_css_color("#5865f280"),
            Some(Color32::from_rgba_unmultiplied(0x58, 0x65, 0xf2, 0x80))
        );
    }

    #[test]
    fn parses_rgb_and_rgba() {
        assert_eq!(
            parse_css_color("rgb(88, 101, 242)"),
            Some(Color32::from_rgb(88, 101, 242))
        );
        assert_eq!(
            parse_css_color("rgba(88, 101, 242, 0.5)"),
            Some(Color32::from_rgba_unmultiplied(88, 101, 242, 127))
        );
    }

    #[test]
    fn hex_roundtrip() {
        let color = Color32::from_rgb(0x58, 0x65, 0xf2);
        assert_eq!(color_to_hex(color), "#5865f2");
        assert_eq!(parse_css_color(&color_to_hex(color)), Some(color));
    }

    #[test]
    fn contrast_extremes_are_sane() {
        let white = Color32::WHITE;
        let black = Color32::BLACK;
        assert!((contrast_ratio(white, black) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 0.001);
    }

    #[test]
    fn readable_on_picks_the_contrasting_ink() {
        assert_eq!(readable_on(Color32::from_rgb(20, 20, 20)), Color32::WHITE);
        assert_eq!(
            readable_on(Color32::from_rgb(240, 240, 240)),
            Color32::BLACK
        );
    }

    #[test]
    fn formats_duration() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65_000), "1:05");
    }
}
