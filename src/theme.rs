use ratatui::style::Color;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Minimal terminal palette. With pywal present (`~/.cache/wal/colors.json`),
/// the wallpaper-generated palette drives the entire interface; otherwise the
/// terminal defaults are used with a single cyan accent.
pub const DEFAULT_TEXT: Color = Color::Reset;
pub const DEFAULT_MUTED: Color = Color::DarkGray;
pub const DEFAULT_ACCENT: Color = Color::Cyan;
pub const DEFAULT_SUCCESS: Color = Color::Green;
pub const DEFAULT_WARN: Color = Color::Yellow;
pub const DEFAULT_ERROR: Color = Color::Red;

struct Theme {
    text: Color,
    muted: Color,
    accent: Color,
    /// Second accent (pywal `color6`) — ytmtui's `accent2`, used for the
    /// decorative rule under the search input.
    accent2: Color,
    success: Color,
    warn: Color,
    error: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: DEFAULT_TEXT,
            muted: DEFAULT_MUTED,
            accent: DEFAULT_ACCENT,
            accent2: DEFAULT_ACCENT,
            success: DEFAULT_SUCCESS,
            warn: DEFAULT_WARN,
            error: DEFAULT_ERROR,
        }
    }
}

static THEME: LazyLock<Theme> = LazyLock::new(Theme::build);

pub static TEXT: LazyLock<Color> = LazyLock::new(|| THEME.text);
pub static MUTED: LazyLock<Color> = LazyLock::new(|| THEME.muted);
pub static ACCENT: LazyLock<Color> = LazyLock::new(|| THEME.accent);
pub static ACCENT2: LazyLock<Color> = LazyLock::new(|| THEME.accent2);
pub static SUCCESS: LazyLock<Color> = LazyLock::new(|| THEME.success);
pub static WARN: LazyLock<Color> = LazyLock::new(|| THEME.warn);
pub static ERROR: LazyLock<Color> = LazyLock::new(|| THEME.error);

impl Theme {
    fn build() -> Self {
        match wal_palette() {
            Some(json) => Self::from_json(&json),
            None => Self::default(),
        }
    }

    /// Maps a parsed pywal palette onto the interface colors. There is no
    /// background mapping on purpose — the frame never paints a background,
    /// so the terminal's own surface (transparency, blur) shows through.
    fn from_json(json: &Value) -> Self {
        let Some(colors) = json.get("colors") else {
            return Self::default();
        };
        let hex = |name: &str| decode_hex(colors.get(name)?.as_str()?);
        if hex("color0").is_none() {
            return Self::default();
        }
        let rgb = |t: Option<(u8, u8, u8)>, fallback: Color| match t {
            Some((r, g, b)) => Color::Rgb(r, g, b),
            None => fallback,
        };
        let text = hex("color7")
            .or_else(|| hex("color15"))
            .or_else(|| decode_hex(json.pointer("/special/foreground")?.as_str()?));
        Self {
            text: rgb(text, DEFAULT_TEXT),
            muted: rgb(hex("color8"), DEFAULT_MUTED),
            accent: rgb(hex("color4").or_else(|| hex("color6")), DEFAULT_ACCENT),
            accent2: rgb(hex("color6").or_else(|| hex("color14")), DEFAULT_ACCENT),
            success: rgb(hex("color2"), DEFAULT_SUCCESS),
            warn: rgb(hex("color3"), DEFAULT_WARN),
            error: rgb(hex("color1"), DEFAULT_ERROR),
        }
    }
}

/// Parse pywal's `#rrggbb` entries.
fn decode_hex(raw: &str) -> Option<(u8, u8, u8)> {
    let trimmed = raw.trim_start_matches('#');
    if trimmed.len() < 6 {
        return None;
    }
    Some((
        u8::from_str_radix(trimmed.get(0..2)?, 16).ok()?,
        u8::from_str_radix(trimmed.get(2..4)?, 16).ok()?,
        u8::from_str_radix(trimmed.get(4..6)?, 16).ok()?,
    ))
}

/// Read pywal's cached palette, written on every `wal` run.
fn wal_palette() -> Option<Value> {
    let path = match std::env::var_os("WAL_COLORS_FILE") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var_os("HOME")?)
            .join(".cache")
            .join("wal")
            .join("colors.json"),
    };
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "special": {"background": "#1b1d1e", "foreground": "#f8f8f8"},
            "colors": {
                "color1": "#d64d4d", "color2": "#52a452", "color3": "#c8a03c",
                "color4": "#4b8bc9", "color6": "#d0ccb4", "color7": "#a9a9a9",
                "color8": "#555555", "color0": "#202124"
            }
        })
    }

    #[test]
    fn pywal_palette_maps_to_theme() {
        let theme = Theme::from_json(&sample());
        assert_eq!(theme.accent, Color::Rgb(0x4b, 0x8b, 0xc9));
        assert_eq!(theme.accent2, Color::Rgb(0xd0, 0xcc, 0xb4));
        assert_eq!(theme.error, Color::Rgb(0xd6, 0x4d, 0x4d));
        assert_eq!(theme.warn, Color::Rgb(0xc8, 0xa0, 0x3c));
        assert_eq!(theme.success, Color::Rgb(0x52, 0xa4, 0x52));
        assert_eq!(theme.muted, Color::Rgb(0x55, 0x55, 0x55));
        assert_eq!(theme.text, Color::Rgb(0xa9, 0xa9, 0xa9));
    }

    #[test]
    fn missing_palette_falls_back_to_terminal_defaults() {
        let theme = Theme::from_json(&Value::default());
        assert_eq!(theme.accent, DEFAULT_ACCENT);
        assert_eq!(theme.accent2, DEFAULT_ACCENT);
        assert_eq!(theme.text, DEFAULT_TEXT);
    }
}
