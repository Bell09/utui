use crate::util::{now_unix, write_atomic};
use crate::yt::Video;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const MAX_SAVED: usize = 1_000;
const MAX_HISTORY: usize = 250;
const MAX_SEARCHES: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub video: Video,
    pub watched_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub saved: Vec<Video>,
    pub liked: Vec<Video>,
    pub watch_later: Vec<Video>,
    pub history: Vec<HistoryEntry>,
    /// Newest first.
    pub search_history: Vec<String>,
}

impl Store {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("could not read library {}", path.display()))?;
        let mut store: Self = serde_json::from_str(&text)
            .with_context(|| format!("could not parse library {}", path.display()))?;
        store.normalize();
        Ok(store)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = serde_json::to_string_pretty(self).context("could not serialize library")?;
        write_atomic(path, &body)
    }

    pub fn normalize(&mut self) {
        deduplicate_videos(&mut self.saved);
        self.saved.truncate(MAX_SAVED);
        deduplicate_videos(&mut self.liked);
        self.liked.truncate(MAX_SAVED);
        deduplicate_videos(&mut self.watch_later);
        self.watch_later.truncate(MAX_SAVED);

        let mut seen = std::collections::HashSet::new();
        self.history
            .retain(|entry| seen.insert(entry.video.id.clone()));
        self.history
            .sort_by_key(|entry| std::cmp::Reverse(entry.watched_at));
        self.history.truncate(MAX_HISTORY);

        let mut seen = std::collections::HashSet::new();
        self.search_history
            .retain(|query| seen.insert(query.to_lowercase()));
        self.search_history.truncate(MAX_SEARCHES);
    }

    pub fn is_saved(&self, id: &str) -> bool {
        self.saved.iter().any(|video| video.id == id)
    }

    /// Adds or removes a video. Returns true when the video is now saved.
    pub fn toggle_saved(&mut self, video: &Video) -> bool {
        if let Some(position) = self.saved.iter().position(|saved| saved.id == video.id) {
            self.saved.remove(position);
            false
        } else {
            self.saved.insert(0, video.clone());
            self.saved.truncate(MAX_SAVED);
            true
        }
    }

    pub fn is_liked(&self, id: &str) -> bool {
        self.liked.iter().any(|video| video.id == id)
    }

    pub fn is_watch_later(&self, id: &str) -> bool {
        self.watch_later.iter().any(|video| video.id == id)
    }

    /// Forces the liked-mirror into an exact state (used after account sync).
    pub fn set_like(&mut self, video: &Video, on: bool) {
        self.liked.retain(|liked| liked.id != video.id);
        if on {
            self.liked.insert(0, video.clone());
            self.liked.truncate(MAX_SAVED);
        }
    }

    /// Forces the Watch Later mirror into an exact state (used after account sync).
    pub fn set_watch_later(&mut self, video: &Video, on: bool) {
        self.watch_later.retain(|queued| queued.id != video.id);
        if on {
            self.watch_later.insert(0, video.clone());
            self.watch_later.truncate(MAX_SAVED);
        }
    }

    pub fn record_watch(&mut self, video: &Video) {
        self.history.retain(|entry| entry.video.id != video.id);
        self.history.insert(
            0,
            HistoryEntry {
                video: video.clone(),
                watched_at: now_unix(),
            },
        );
        self.history.truncate(MAX_HISTORY);
    }

    pub fn add_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.search_history
            .retain(|existing| !existing.eq_ignore_ascii_case(query));
        self.search_history.insert(0, query.to_string());
        self.search_history.truncate(MAX_SEARCHES);
    }

    /// Resolve `!1`, `!2`, ... against newest-first search history.
    pub fn recall_search(&self, input: &str) -> String {
        resolve_recall(input, &self.search_history)
    }
}

pub fn resolve_recall(input: &str, history: &[String]) -> String {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix('!') else {
        return trimmed.to_string();
    };
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return trimmed.to_string();
    }
    let Ok(index) = digits.parse::<usize>() else {
        return trimmed.to_string();
    };
    if index == 0 {
        return trimmed.to_string();
    }
    let Some(recalled) = history.get(index - 1) else {
        return trimmed.to_string();
    };
    let suffix = rest[digits.len()..].trim();
    if suffix.is_empty() {
        recalled.clone()
    } else {
        format!("{recalled} {suffix}")
    }
}

fn deduplicate_videos(videos: &mut Vec<Video>) {
    let mut seen = std::collections::HashSet::new();
    videos.retain(|video| seen.insert(video.id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recalls_history_entries() {
        let history = vec!["rust".to_string(), "lofi".to_string()];
        assert_eq!(resolve_recall("!1", &history), "rust");
        assert_eq!(resolve_recall("!2 beats", &history), "lofi beats");
        assert_eq!(resolve_recall("!9", &history), "!9");
        assert_eq!(resolve_recall("plain", &history), "plain");
    }

    #[test]
    fn search_history_is_newest_first_and_deduplicated() {
        let mut store = Store::default();
        store.add_search("rust");
        store.add_search("lofi");
        store.add_search("RUST");
        assert_eq!(store.search_history, vec!["RUST", "lofi"]);
    }

    #[test]
    fn set_like_forces_exact_state() {
        let mut store = Store::default();
        let video = Video {
            id: "abc".to_string(),
            title: "Abc".to_string(),
            ..Video::default()
        };
        store.set_like(&video, true);
        assert!(store.is_liked("abc"));
        store.set_like(&video, true);
        assert_eq!(store.liked.len(), 1, "idempotent set must not duplicate");
        store.set_like(&video, false);
        assert!(!store.is_liked("abc"));
        assert!(store.liked.is_empty());
    }

    #[test]
    fn set_watch_later_forces_exact_state() {
        let mut store = Store::default();
        let queued = Video {
            id: "wl".to_string(),
            ..Video::default()
        };
        let other = Video {
            id: "wl2".to_string(),
            ..Video::default()
        };
        store.set_watch_later(&queued, true);
        store.set_watch_later(&other, true);
        assert_eq!(
            store
                .watch_later
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>(),
            vec!["wl2", "wl"],
            "newest first"
        );
        store.set_watch_later(&queued, false);
        assert!(!store.is_watch_later("wl"));
        assert_eq!(store.watch_later.len(), 1);
    }
}
