use egui::Color32;

/// Return the first grapheme-ish character of a display name, uppercased.
pub fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string())
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
    fn formats_duration() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65_000), "1:05");
    }
}
