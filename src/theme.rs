use ratatui::style::Color;

/// Minimal terminal palette: rely on the user's terminal foreground/background
/// and use a single accent plus semantic states.
pub const TEXT: Color = Color::Reset;
pub const MUTED: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Cyan;
pub const SUCCESS: Color = Color::Green;
pub const WARN: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
