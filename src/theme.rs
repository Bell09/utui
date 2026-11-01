//! Colour theme: a 16-colour palette detected from the desktop's wallpaper
//! theming tool, mapped onto semantic UI roles.
//!
//! pywal, wallust and matugen all produce compatible palettes. We look in
//! their cache/config directories (and at JSON template outputs declared in
//! their configs), pick the newest valid palette, and watch it live so a
//! wallpaper change repaints the running TUI. If none is present we fall
//! back to the terminal's own colours with a single accent.
//!
//! There is deliberately no background mapping — the frame never paints a
//! background, so the terminal's own surface (transparency, blur) shows
//! through.

use ratatui::style::Color;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Terminal defaults: used when no palette file exists, and as per-colour
/// fallbacks when a palette lacks a slot.
pub const DEFAULT_TEXT: Color = Color::Reset;
pub const DEFAULT_MUTED: Color = Color::DarkGray;
pub const DEFAULT_ACCENT: Color = Color::Cyan;
pub const DEFAULT_SUCCESS: Color = Color::Green;
pub const DEFAULT_WARN: Color = Color::Yellow;
pub const DEFAULT_ERROR: Color = Color::Red;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Primary text (color7).
    pub text: Color,
    /// Secondary text: meta, hairlines, status (color8).
    pub muted: Color,
    /// Primary accent: prefixes, focused keys (color4).
    pub accent: Color,
    /// Second accent (color6) — the decorative rule under the search input.
    pub accent2: Color,
    /// The app name in the header (color13).
    pub brand: Color,
    /// Success messages (color2).
    pub success: Color,
    /// Warnings (color3).
    pub warn: Color,
    /// Errors (color1).
    pub error: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            text: DEFAULT_TEXT,
            muted: DEFAULT_MUTED,
            accent: DEFAULT_ACCENT,
            accent2: DEFAULT_ACCENT,
            brand: DEFAULT_ACCENT,
            success: DEFAULT_SUCCESS,
            warn: DEFAULT_WARN,
            error: DEFAULT_ERROR,
        }
    }
}

impl Theme {
    /// Load the newest palette, returning the file it came from so the live
    /// watcher can follow it.
    pub fn load_with_source() -> Option<(Self, PathBuf)> {
        for (path, _) in palette_files_by_mtime() {
            if let Some(theme) = Self::reload_from(&path) {
                return Some((theme, path));
            }
        }
        None
    }

    /// The newest palette file that actually parses. Used by the poller;
    /// validation is cached by mtime, so a file being rewritten in place is
    /// skipped until the write finishes without re-parsing every tick.
    pub fn source_file() -> Option<PathBuf> {
        palette_files_by_mtime()
            .into_iter()
            .find(|(path, mtime)| is_valid_palette(path, *mtime))
            .map(|(path, _)| path)
    }

    /// The newest existing palette candidate, whether or not it parses right
    /// now. The poller uses this to distinguish "no theming tool at all" from
    /// "the newest file is still being written".
    pub fn latest_file() -> Option<PathBuf> {
        palette_files_by_mtime()
            .into_iter()
            .next()
            .map(|(path, _)| path)
    }

    /// Re-read one specific palette file. `None` when it can't be parsed:
    /// tools rewrite their cache in place, so a read racing the write sees a
    /// partial file — callers keep the current theme instead of flashing to
    /// the fallback, and retry once the write lands.
    pub fn reload_from(path: &Path) -> Option<Self> {
        load_file(path).map(Self::from_palette)
    }

    /// Map a 16-colour palette onto the semantic roles. colour0 is unused
    /// (there is no background paint); colour5 and colour14 currently have
    /// no role either.
    fn from_palette(p: [Color; 16]) -> Self {
        Self {
            text: p[7],
            muted: p[8],
            accent: p[4],
            accent2: p[6],
            brand: p[13],
            success: p[2],
            warn: p[3],
            error: p[1],
        }
    }
}

/// Candidate palette files from the common wallpaper tools.
///
/// The paths derive from XDG directories and config files that do not move
/// while the app runs, so build them once. `palette_files_by_mtime` still
/// stats every candidate, but it no longer rebuilds and re-deduplicates the
/// path list on each poll.
fn palette_files() -> &'static [PathBuf] {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut files = Vec::new();

            for base in xdg_dirs("XDG_CACHE_HOME", ".cache") {
                for tool in ["wal", "wallust", "matugen"] {
                    let dir = base.join(tool);
                    files.push(dir.join("colors.json"));
                    files.push(dir.join("colors"));
                }
            }
            for base in xdg_dirs("XDG_CONFIG_HOME", ".config") {
                files.push(base.join("matugen").join("colors.json"));
                files.push(base.join("wallust").join("colors.json"));
            }
            for base in xdg_dirs("XDG_DATA_HOME", ".local/share") {
                files.push(base.join("matugen").join("colors.json"));
            }

            files.extend(tool_config_outputs());
            dedup(files)
        })
        .as_slice()
}

/// Existing candidates, newest first. Ties are broken by path so selection is
/// deterministic when two tools write in the same timestamp tick.
fn palette_files_by_mtime() -> Vec<(PathBuf, SystemTime)> {
    let mut found = Vec::new();
    for path in palette_files() {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        found.push((path.clone(), mtime));
    }
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found
}

/// Output files declared in matugen/wallust configs, filtered to formats a
/// palette can live in. Read once at startup: new output paths need a restart,
/// while palette contents keep reloading live.
fn tool_config_outputs() -> Vec<PathBuf> {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut outputs = Vec::new();
            for base in xdg_dirs("XDG_CONFIG_HOME", ".config") {
                outputs.extend(config_outputs(
                    &base.join("matugen").join("config.toml"),
                    &["output_path"],
                ));
                outputs.extend(config_outputs(
                    &base.join("wallust").join("wallust.toml"),
                    &["target", "dst"],
                ));
            }
            dedup(outputs)
        })
        .clone()
}

/// Pull output paths out of the `[templates]` section (and wallust's old
/// `[[entry]]` sections). This is intentionally a small TOML-line scanner
/// rather than a full parser: template output paths are simple quoted strings.
fn config_outputs(path: &Path, keys: &[&str]) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Some(config_dir) = path.parent() else {
        return Vec::new();
    };

    let mut outputs = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let header = line.trim_start_matches('[');
            section = header
                .split(']')
                .next()
                .unwrap_or(header)
                .trim()
                .to_ascii_lowercase();
            continue;
        }

        let in_templates =
            section == "templates" || section.starts_with("templates.") || section == "entry";
        if !in_templates {
            continue;
        }

        for key in keys {
            let Some(value) = find_assignment(line, key) else {
                continue;
            };
            let Some(raw_path) = toml_string(value) else {
                continue;
            };
            if let Some(output) = resolve_output_path(&raw_path, config_dir) {
                outputs.push(output);
            }
        }
    }
    outputs
}

/// Find `key = ...` (or `entry.key = ...`) and return everything after `=`.
fn find_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(offset) = line[from..].find(key) {
        let index = from + offset;
        let boundary = match line[..index].chars().next_back() {
            None => true,
            Some(ch) => ch.is_whitespace() || matches!(ch, '{' | ',' | '.'),
        };
        if boundary {
            let rest = line[index + key.len()..].trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                return Some(value.trim_start());
            }
        }
        from = index + key.len();
    }
    None
}

/// Extract a quoted TOML string, ignoring an inline comment after it.
fn toml_string(value: &str) -> Option<String> {
    let value = value.trim_start();
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let rest = &value[1..];
    let mut escaped = false;
    for (index, ch) in rest.char_indices() {
        if quote == '"' && ch == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if ch == quote && !escaped {
            return Some(rest[..index].to_string());
        }
        escaped = false;
    }
    None
}

/// Expand `~` and resolve relative output paths against the config's folder.
/// Only JSON files (and extensionless `colors` files) can be palettes.
fn resolve_output_path(raw: &str, config_dir: &Path) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let path = if raw == "~" {
        home()?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home()?.join(rest)
    } else if let Some(rest) = raw.strip_prefix("$HOME/") {
        home()?.join(rest)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        config_dir.join(raw)
    };

    let is_json = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    let is_colors = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "colors");
    (is_json || is_colors).then_some(path)
}

fn xdg_dirs(variable: &str, fallback: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(value) = std::env::var(variable) {
        if !value.is_empty() {
            dirs.push(PathBuf::from(value));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let default = PathBuf::from(home).join(fallback);
        if !dirs.contains(&default) {
            dirs.push(default);
        }
    }
    dirs
}

fn dedup(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths.drain(..) {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

/// Cached parse success, keyed by file mtime. Failed parses are deliberately
/// not cached: a writer can finish within the same timestamp tick, and we want
/// the next poll to retry rather than ignore the finished palette forever.
fn is_valid_palette(path: &Path, mtime: SystemTime) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, SystemTime>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if cache.get(path) == Some(&mtime) {
        return true;
    }

    let valid = load_file(path).is_some();
    if valid {
        cache.insert(path.to_path_buf(), mtime);
    } else {
        cache.remove(path);
    }
    valid
}

/// Parse one palette file, trying the JSON forms first.
fn load_file(path: &Path) -> Option<[Color; 16]> {
    load_json(path).or_else(|| load_plain(path))
}

/// Parse a pywal/wallust/Material-role/base16 JSON palette.
fn load_json(path: &Path) -> Option<[Color; 16]> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;

    ansi_palette(&value)
        .or_else(|| base16_palette(&value))
        .or_else(|| matugen_palette(&value))
}

/// Parse the canonical pywal/wallust form: `colors.color0`..`color15`, with
/// optional `special.background`/`special.foreground` fallbacks.
fn ansi_palette(value: &Value) -> Option<[Color; 16]> {
    let colors = value.get("colors").unwrap_or(value);
    let special = value.get("special").and_then(Value::as_object);

    let mut palette = [Color::Indexed(0); 16];
    let mut found = false;

    if let Some(object) = colors.as_object() {
        for (i, slot) in palette.iter_mut().enumerate() {
            let key = format!("color{i}");
            let from_colors = object.get(&key).and_then(json_color);
            let from_special = match i {
                0 | 7 => special
                    .and_then(|s| s.get(if i == 0 { "background" } else { "foreground" }))
                    .and_then(json_color),
                _ => None,
            };
            if let Some(color) = from_colors.or(from_special) {
                *slot = color;
                found = true;
            }
        }
    } else if let Some(array) = colors.as_array() {
        for (i, item) in array.iter().take(16).enumerate() {
            if let Some(color) = json_color(item) {
                palette[i] = color;
                found = true;
            }
        }
    }

    found.then_some(palette)
}

/// Parse matugen's base16 JSON (base00..base0F) and map it to ANSI roles.
fn base16_palette(value: &Value) -> Option<[Color; 16]> {
    let root = value.get("base16").unwrap_or(value);
    let slots = base16_slots(root.as_object()?)
        .or_else(|| value.get("colors")?.as_object().and_then(base16_slots))?;

    // base16 -> ANSI: 00 bg, 05 fg, 03 dim, 07 bright, 08..0F accents.
    const MAP: [usize; 16] = [0, 8, 11, 10, 13, 14, 12, 5, 3, 8, 11, 10, 13, 14, 12, 7];
    let mut palette = [Color::Indexed(0); 16];
    for (ansi, base) in MAP.iter().enumerate() {
        palette[ansi] = slots[*base].unwrap_or(Color::Indexed(ansi as u8));
    }
    Some(palette)
}

fn base16_slots(object: &Map<String, Value>) -> Option<[Option<Color>; 16]> {
    if !object.keys().any(|key| key.eq_ignore_ascii_case("base00")) {
        return None;
    }

    let mut slots = [None; 16];
    for (i, slot) in slots.iter_mut().enumerate() {
        let key = format!("base{i:02X}");
        *slot = object
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&key))
            .and_then(|(_, value)| json_color(value));
    }

    // A partial or unrelated object should not masquerade as a palette.
    if slots.iter().filter(|slot| slot.is_some()).count() < 8 {
        return None;
    }
    Some(slots)
}

/// Parse matugen's Material role JSON, flat or nested under `colors` /
/// `schemes.dark` / `schemes.light`.
fn matugen_palette(value: &Value) -> Option<[Color; 16]> {
    role_candidates(value)
        .into_iter()
        .find_map(roles_to_palette)
}

fn role_candidates(value: &Value) -> Vec<&Map<String, Value>> {
    fn walk<'a>(value: &'a Value, depth: usize, out: &mut Vec<&'a Map<String, Value>>) {
        let Some(object) = value.as_object() else {
            return;
        };
        out.push(object);
        if depth == 0 {
            return;
        }
        for key in ["colors", "schemes", "palettes", "dark", "light", "default"] {
            if let Some(child) = object.get(key) {
                walk(child, depth - 1, out);
            }
        }
    }

    let mut out = Vec::new();
    walk(value, 3, &mut out);
    out
}

fn roles_to_palette(object: &Map<String, Value>) -> Option<[Color; 16]> {
    let background = role(object, &["background", "surface", "surface_dim"])?;
    let foreground = role(object, &["on_background", "on_surface", "foreground"])?;
    let primary = role(object, &["primary"])?;

    let secondary = role(object, &["secondary"]).unwrap_or(primary);
    let tertiary = role(object, &["tertiary"]).unwrap_or(secondary);
    let error = role(object, &["error"]).unwrap_or(primary);
    let outline =
        role(object, &["outline", "outline_variant", "surface_variant"]).unwrap_or(foreground);
    let bright = role(object, &["on_surface", "on_background"]).unwrap_or(foreground);

    let mut palette = [Color::Indexed(0); 16];
    palette[0] = background;
    palette[1] = error;
    palette[2] = tertiary;
    palette[3] = secondary;
    palette[4] = primary;
    palette[5] = tertiary;
    palette[6] = secondary;
    palette[7] = foreground;
    palette[8] = outline;
    palette[9] = error;
    palette[10] = primary;
    palette[11] = secondary;
    palette[12] = primary;
    palette[13] = tertiary;
    palette[14] = secondary;
    palette[15] = bright;
    Some(palette)
}

fn role(object: &Map<String, Value>, keys: &[&str]) -> Option<Color> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_color))
}

/// Read a colour from a JSON string or from matugen's nested
/// `{ "default": { "hex": "#..." } }` / `{ "dark": { "hex": ... } }` forms.
fn json_color(value: &Value) -> Option<Color> {
    match value {
        Value::String(text) => parse_hex(text),
        Value::Object(object) => {
            for key in ["hex", "default", "dark", "light", "value", "color"] {
                if let Some(child) = object.get(key) {
                    if let Some(color) = json_color(child) {
                        return Some(color);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Parse the plain `colors` file (16 hex lines, `color0`..`color15`).
fn load_plain(path: &Path) -> Option<[Color; 16]> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut palette = [Color::Indexed(0); 16];
    let mut found = false;
    for (i, hex) in text.lines().enumerate().take(16) {
        if let Some(color) = parse_hex(hex.trim()) {
            palette[i] = color;
            found = true;
        }
    }
    found.then_some(palette)
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 && s.len() != 3 {
        return None;
    }
    let expand = |c: char| -> u8 {
        let v = c.to_digit(16).unwrap_or(0) as u8;
        (v << 4) | v
    };
    let (r, g, b) = if s.len() == 6 {
        (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        )
    } else {
        (
            expand(s.chars().next()?),
            expand(s.chars().nth(1)?),
            expand(s.chars().nth(2)?),
        )
    };
    Some(Color::Rgb(r, g, b))
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
                "color8": "#555555", "color13": "#7ec8a0", "color0": "#202124"
            }
        })
    }

    #[test]
    fn pywal_palette_maps_to_theme() {
        let palette = ansi_palette(&sample()).expect("pywal palette parses");
        let theme = Theme::from_palette(palette);
        assert_eq!(theme.accent, Color::Rgb(0x4b, 0x8b, 0xc9));
        assert_eq!(theme.accent2, Color::Rgb(0xd0, 0xcc, 0xb4));
        assert_eq!(theme.brand, Color::Rgb(0x7e, 0xc8, 0xa0));
        assert_eq!(theme.error, Color::Rgb(0xd6, 0x4d, 0x4d));
        assert_eq!(theme.warn, Color::Rgb(0xc8, 0xa0, 0x3c));
        assert_eq!(theme.success, Color::Rgb(0x52, 0xa4, 0x52));
        assert_eq!(theme.muted, Color::Rgb(0x55, 0x55, 0x55));
        assert_eq!(theme.text, Color::Rgb(0xa9, 0xa9, 0xa9));
    }

    #[test]
    fn matugen_role_json_maps_to_theme() {
        let value = serde_json::json!({
            "colors": {
                "background": "#101214",
                "on_background": "#e7e2d6",
                "primary": "#c7a6ff",
                "secondary": "#8fd3c4",
                "tertiary": "#ffc09f",
                "error": "#ff6b6b",
                "outline": "#5c5a54"
            }
        });
        let palette = matugen_palette(&value).expect("matugen roles parse");
        let theme = Theme::from_palette(palette);
        assert_eq!(theme.text, Color::Rgb(0xe7, 0xe2, 0xd6));
        assert_eq!(theme.accent, Color::Rgb(0xc7, 0xa6, 0xff));
        assert_eq!(theme.accent2, Color::Rgb(0x8f, 0xd3, 0xc4));
        assert_eq!(theme.error, Color::Rgb(0xff, 0x6b, 0x6b));
    }

    #[test]
    fn matugen_base16_json_maps_to_theme() {
        let hexes: Vec<String> = (0..16)
            .map(|i| format!("#{:02x}{:02x}{:02x}", i * 15, 100, 200))
            .collect();
        let value = serde_json::json!({
            "base16": {
                "base00": hexes[0], "base01": hexes[1], "base02": hexes[2],
                "base03": hexes[3], "base04": hexes[4], "base05": hexes[5],
                "base06": hexes[6], "base07": hexes[7], "base08": hexes[8],
                "base09": hexes[9], "base0A": hexes[10], "base0B": hexes[11],
                "base0C": hexes[12], "base0D": hexes[13], "base0E": hexes[14],
                "base0F": hexes[15]
            }
        });
        let palette = base16_palette(&value).expect("base16 parses");
        let theme = Theme::from_palette(palette);
        // MAP: text = base05, accent = base0D, accent2 = base0C, error = base08.
        assert_eq!(theme.text, Color::Rgb(5 * 15, 100, 200));
        assert_eq!(theme.accent, Color::Rgb(13 * 15, 100, 200));
        assert_eq!(theme.accent2, Color::Rgb(12 * 15, 100, 200));
        assert_eq!(theme.error, Color::Rgb(8 * 15, 100, 200));
    }

    #[test]
    fn empty_json_is_not_a_palette() {
        assert!(ansi_palette(&Value::default()).is_none());
        assert!(matugen_palette(&Value::default()).is_none());
        assert!(base16_palette(&Value::default()).is_none());
    }

    #[test]
    fn fallback_uses_terminal_defaults() {
        let theme = Theme::default();
        assert_eq!(theme.accent, DEFAULT_ACCENT);
        assert_eq!(theme.accent2, DEFAULT_ACCENT);
        assert_eq!(theme.text, DEFAULT_TEXT);
    }

    #[test]
    fn tool_config_outputs_scan_declared_templates() {
        let root = std::env::temp_dir().join(format!("utui-theme-{}", std::process::id()));
        std::fs::create_dir_all(root.join("matugen")).unwrap();
        std::fs::write(
            root.join("matugen/config.toml"),
            "[templates]\nmy_tmpl = {\n  output_path = '~/out/colors.json'\n}\n",
        )
        .unwrap();
        let outputs = config_outputs(&root.join("matugen/config.toml"), &["output_path"]);
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        assert_eq!(outputs, vec![home.join("out/colors.json")]);
        std::fs::remove_dir_all(&root).ok();
    }
}
