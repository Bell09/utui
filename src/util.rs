use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub fn format_duration(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "—".to_string();
    };
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn format_views(views: Option<u64>) -> String {
    let Some(views) = views else {
        return "—".to_string();
    };
    if views >= 1_000_000_000 {
        format!("{:.1}B", views as f64 / 1_000_000_000.0)
    } else if views >= 1_000_000 {
        format!("{:.1}M", views as f64 / 1_000_000.0)
    } else if views >= 1_000 {
        format!("{:.1}K", views as f64 / 1_000.0)
    } else {
        views.to_string()
    }
}

/// Human-friendly age, based on whole seconds.
pub fn relative_time(timestamp: Option<i64>) -> String {
    relative_time_at(timestamp, now_unix())
}

pub fn relative_time_at(timestamp: Option<i64>, now: i64) -> String {
    let Some(timestamp) = timestamp.filter(|timestamp| *timestamp > 0) else {
        return "unknown".to_string();
    };
    let delta = now.saturating_sub(timestamp);
    if delta < 0 {
        return "scheduled".to_string();
    }
    let minute = 60;
    let hour = 60 * minute;
    let day = 24 * hour;
    let week = 7 * day;
    let month = 30 * day;
    let year = 365 * day;

    if delta < minute {
        "just now".to_string()
    } else if delta < hour {
        format!("{}m ago", delta / minute)
    } else if delta < day {
        format!("{}h ago", delta / hour)
    } else if delta < week {
        format!("{}d ago", delta / day)
    } else if delta < month {
        format!("{}w ago", delta / week)
    } else if delta < year {
        format!("{}mo ago", delta / month)
    } else {
        format!("{}y ago", delta / year)
    }
}

/// UTC calendar date derived from a Unix timestamp.
pub fn format_date(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp.filter(|timestamp| *timestamp > 0) else {
        return "unknown".to_string();
    };
    let days = timestamp.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

/// Clip a string to a display width, using an ellipsis when needed.
pub fn truncate(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if value.width() <= max_width {
        return value.to_string();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let mut output = String::new();
    let mut width = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width >= max_width {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output.push('…');
    output
}

pub fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// Read a percentage from a yt-dlp progress line.
pub fn parse_percent(line: &str) -> Option<u8> {
    let percent_position = line.rfind('%')?;
    let before = &line[..percent_position];
    let token = before.split_whitespace().last()?;
    let value: f64 = token.parse().ok()?;
    Some(value.clamp(0.0, 100.0).round() as u8)
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create directory {}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, contents)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

pub fn copy_to_clipboard(value: &str) -> Result<()> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];

    for (program, args) in candidates {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            if stdin.write_all(value.as_bytes()).is_err() {
                continue;
            }
        }
        if child.wait().map(|status| status.success()).unwrap_or(false) {
            return Ok(());
        }
    }

    anyhow::bail!("no clipboard tool found (install wl-clipboard, xclip, or xsel)")
}

pub fn open_external(target: &str) -> Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("could not launch {program}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(Some(9)), "0:09");
        assert_eq!(format_duration(Some(65)), "1:05");
        assert_eq!(format_duration(Some(3_661)), "1:01:01");
        assert_eq!(format_duration(None), "—");
    }

    #[test]
    fn formats_views() {
        assert_eq!(format_views(Some(999)), "999");
        assert_eq!(format_views(Some(1_500)), "1.5K");
        assert_eq!(format_views(Some(2_300_000)), "2.3M");
        assert_eq!(format_views(None), "—");
    }

    #[test]
    fn formats_dates() {
        assert_eq!(format_date(Some(0)), "unknown");
        assert_eq!(format_date(Some(1_767_225_600)), "2026-01-01");
    }

    #[test]
    fn parses_progress() {
        assert_eq!(
            parse_percent("[download]  42.7% of 10.00MiB at 1.00MiB/s ETA 00:05"),
            Some(43)
        );
        assert_eq!(parse_percent("[download] Destination: file.mp4"), None);
    }

    #[test]
    fn truncates_wide_text() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("日本語", 4), "日…");
    }

    #[test]
    fn relative_ages() {
        let now = 1_700_000_000;
        assert_eq!(relative_time_at(Some(now - 30), now), "just now");
        assert_eq!(relative_time_at(Some(now - 120), now), "2m ago");
        assert_eq!(relative_time_at(Some(now - 7_200), now), "2h ago");
    }
}
