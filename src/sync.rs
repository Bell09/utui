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

/// Outcome of the helper's `check` mode: whether the browser's cookie jar
/// carries a signed-in YouTube session.
#[derive(Debug, Clone, PartialEq)]
pub enum Session {
    /// A YouTube session (SAPISID cookie) was found in the jar.
    SignedIn,
    /// Cookies read fine, but there is no YouTube session in them.
    SignedOut(String),
    /// The jar could not be read (locked profile, missing Python helper, …).
    Unknown(String),
}

/// Read the cookie jar offline (no network) and classify the session.
///
/// A failure to *read* is deliberately not the same as being signed out:
/// a locked Chrome database or a missing Python `yt-dlp` module says
/// nothing about the account state.
pub async fn session_check(browser: &str, script: &Path) -> Result<Session> {
    let (stdout, stderr) = invoke(script, &[browser.to_string(), "check".to_string()]).await?;
    Ok(parse_check(&stdout, &stderr))
}

/// Classify one helper `check` result; the streams are passed separately so
/// the mapping is unit-testable without Python.
pub fn parse_check(stdout: &str, stderr: &str) -> Session {
    let value: Option<serde_json::Value> = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok());
    match value {
        Some(json) if json.get("ok").and_then(serde_json::Value::as_bool) == Some(true) => {
            Session::SignedIn
        }
        Some(json) => {
            let detail = json
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            if detail.starts_with("not signed in") {
                Session::SignedOut(detail)
            } else {
                Session::Unknown(detail)
            }
        }
        None => {
            let detail = if stderr.contains("No module named") && stderr.contains("yt_dlp") {
                "offline check needs Python's yt-dlp (pip install yt-dlp); \
                 the live check can still verify the session"
                    .to_string()
            } else if stderr.trim().is_empty() {
                "the cookie helper printed no result".to_string()
            } else {
                format!(
                    "the cookie helper failed: {}",
                    stderr.lines().last().unwrap_or_default().trim()
                )
            };
            Session::Unknown(detail)
        }
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

/// Run the helper once and hand back its raw streams.
async fn invoke(script: &Path, args: &[String]) -> Result<(String, String)> {
    let output = Command::new("python3")
        .arg(script)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("python3 could not be started (is Python with yt-dlp installed?)")?;
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Run one sync action and return the helper's success summary.
pub async fn run(action: &SyncAction, browser: &str, script: &Path) -> Result<String> {
    let (stdout, stderr) = invoke(script, &action.argv(browser)).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_classifies_helper_results() {
        assert_eq!(
            parse_check(r#"{"ok":true,"signed_in":true}"#, ""),
            Session::SignedIn
        );
        assert_eq!(
            parse_check(
                r#"{"ok":false,"error":"not signed in: no SAPISID cookie found in the browser jar"}"#,
                ""
            ),
            Session::SignedOut(
                "not signed in: no SAPISID cookie found in the browser jar".to_string()
            )
        );
        // A jar that cannot be read is not proof of being signed out.
        assert_eq!(
            parse_check(
                r#"{"ok":false,"error":"could not read browser cookies: database is locked"}"#,
                ""
            ),
            Session::Unknown("could not read browser cookies: database is locked".to_string())
        );
        let missing = parse_check(
            "",
            "Traceback (most recent call last):\n  ModuleNotFoundError: No module named 'yt_dlp'",
        );
        let Session::Unknown(detail) = missing else {
            panic!("expected Unknown, got {missing:?}");
        };
        assert!(detail.contains("pip install yt-dlp"), "{detail}");
        let silent = parse_check("", "");
        assert_eq!(
            silent,
            Session::Unknown("the cookie helper printed no result".to_string())
        );
    }
}
