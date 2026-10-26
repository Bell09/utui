//! Account sync: real likes and Watch Later writes against YouTube.
//!
//! Utui talks to YouTube's logged-in web API through a tiny embedded Python
//! helper that reuses yt-dlp's cookie extraction (so every browser/keyring
//! that yt-dlp supports just works) and yt-dlp's HTTP machinery. Results
//! arrive as a single JSON line on stdout.

use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;

pub const SYNC_SCRIPT: &str = include_str!("sync.py");

#[derive(Debug, Clone, PartialEq)]
pub enum SyncAction {
    Like { video_id: String, on: bool },
    WatchLater { video_id: String, add: bool },
}

impl SyncAction {
    fn argv(&self, browser: &str) -> Vec<String> {
        let mut argv = vec![browser.to_string()];
        match self {
            Self::Like { video_id, on } => {
                argv.push("like".into());
                argv.push(video_id.clone());
                argv.push(if *on { "on" } else { "off" }.into());
            }
            Self::WatchLater { video_id, add } => {
                argv.push("watchlater".into());
                argv.push(video_id.clone());
                argv.push(if *add { "add" } else { "remove" }.into());
            }
        }
        argv
    }
}

/// Materialize the embedded helper into the cache directory.
///
/// The on-disk copy keeps invocations debuggable (`python3 sync.py ...` by
/// hand) and is refreshed whenever the embedded script changes.
pub fn prepare_script(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create directory {}", parent.display()))?;
    }
    if path.is_file() {
        if let Ok(existing) = std::fs::read_to_string(path) {
            if existing == SYNC_SCRIPT {
                return Ok(());
            }
        }
    }
    std::fs::write(path, SYNC_SCRIPT).with_context(|| format!("could not write {}", path.display()))
}

/// Run one sync action and return the helper's success summary.
pub async fn run(action: &SyncAction, browser: &str, script: &Path) -> Result<String> {
    let output = Command::new("python3")
        .arg(script)
        .args(action.argv(browser))
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("python3 could not be started (is Python with yt-dlp installed?)")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let result: serde_json::Value = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .context("sync script did not report a result")?;
    if result.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(result
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("synced")
            .to_string());
    }
    let message = result
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown error");
    bail!("{}{}", message, trailing(&stderr))
}

fn trailing(stderr: &str) -> String {
    match stderr.lines().last() {
        Some(line) if !line.is_empty() => format!(" ({line})"),
        _ => String::new(),
    }
}
