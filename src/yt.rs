use crate::util::{fnv1a64, now_unix, write_atomic};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

pub const DEFAULT_SEARCH_SP: &str = "EgIQAQ%253D%253D";
pub const FILTER_HELP: &[(&str, &str)] = &[
    (":hour", "last hour"),
    (":today", "today"),
    (":week", "this week"),
    (":month", "this month"),
    (":year", "this year"),
    (":video", "videos"),
    (":movie", "movies"),
    (":live", "live"),
    (":short", "shorts"),
    (":long", "long videos"),
    (":4k", "4K"),
    (":hd", "HD"),
    (":subtitles", "with subtitles"),
    (":360", "360°"),
    (":vr", "VR"),
    (":3d", "3D"),
    (":hdr", "HDR"),
    (":local", "local"),
    (":newest", "newest first"),
    (":views", "most viewed"),
    (":rating", "highest rated"),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub url: String,
    pub channel: String,
    pub channel_url: Option<String>,
    pub duration: Option<u64>,
    pub view_count: Option<u64>,
    pub timestamp: Option<i64>,
    pub upload_date: Option<String>,
    pub thumbnail: Option<String>,
    pub description: Option<String>,
    pub live: bool,
}

impl Video {
    pub fn is_live(&self) -> bool {
        self.live
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedData {
    pub title: String,
    pub source: String,
    pub items: Vec<Video>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone)]
pub struct FetchSpec {
    pub id: u64,
    pub url: String,
    pub label: String,
    pub limit: usize,
    /// 1-based page window into the list, exactly like yt-x: page N fetches
    /// items `(N-1)*limit+1 ..= N*limit` via `--playlist-start/--playlist-end`.
    pub page: usize,
    pub cookies_browser: Option<String>,
    pub requires_auth: bool,
    pub cache_dir: Option<PathBuf>,
    pub cache_ttl_secs: u64,
    pub bypass_cache: bool,
}

impl FetchSpec {
    pub fn new(id: u64, url: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id,
            url: url.into(),
            label: label.into(),
            limit: 50,
            page: 1,
            cookies_browser: None,
            requires_auth: false,
            cache_dir: None,
            cache_ttl_secs: 600,
            bypass_cache: false,
        }
    }
}

/// The 1-based item window a page covers: page 1 starts at 1, every later
/// page continues where the previous one ended.
fn playlist_window(page: usize, limit: usize) -> (usize, usize) {
    let limit = limit.max(1);
    let start = (page.max(1) - 1) * limit + 1;
    (start, start + limit - 1)
}

#[derive(Debug, Clone)]
pub struct SearchPlan {
    pub term: String,
    pub sp: String,
    pub filter: Option<String>,
}

/// A single colon filter is supported, matching YouTube's own UI model.
/// It can appear at the beginning or end of the query.
pub fn parse_search(input: &str) -> SearchPlan {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut filter_index = None;
    let mut filter = None;

    if let Some(first) = tokens.first() {
        if let Some((name, sp)) = filter_for(first) {
            filter_index = Some(0);
            filter = Some((name.to_string(), sp));
        }
    }

    if filter.is_none() && tokens.len() > 1 {
        if let Some(last) = tokens.last() {
            if let Some((name, sp)) = filter_for(last) {
                filter_index = Some(tokens.len() - 1);
                filter = Some((name.to_string(), sp));
            }
        }
    }

    let term = tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != filter_index)
        .map(|(_, token)| *token)
        .collect::<Vec<_>>()
        .join(" ");

    SearchPlan {
        term,
        sp: filter
            .as_ref()
            .map(|(_, sp)| (*sp).to_string())
            .unwrap_or_else(|| DEFAULT_SEARCH_SP.to_string()),
        filter: filter.map(|(name, _)| name),
    }
}

fn filter_for(token: &str) -> Option<(&'static str, &'static str)> {
    match token.strip_prefix(':')? {
        "hour" => Some(("hour", "EgIIAQ%253D%253D")),
        "today" => Some(("today", "EgIIAg%253D%253D")),
        "week" => Some(("week", "EgIIAw%253D%253D")),
        "month" => Some(("month", "EgIIBA%253D%253D")),
        "year" => Some(("year", "EgIIBQ%253D%253D")),
        "video" => Some(("video", "EgIQAQ%253D%253D")),
        "movie" => Some(("movie", "EgIQBA%253D%253D")),
        "live" => Some(("live", "EgJAAQ%253D%253D")),
        "short" => Some(("short", "EgQQARgB")),
        "long" => Some(("long", "EgQQARgC")),
        "4k" => Some(("4k", "EgJwAQ%253D%253D")),
        "hd" => Some(("hd", "EgIgAQ%253D%253D")),
        "subtitles" => Some(("subtitles", "EgIoAQ%253D%253D")),
        "360" => Some(("360", "EgJ4AQ%253D%253D")),
        "vr" => Some(("vr", "EgLIAQ%253D%253D")),
        "3d" => Some(("3d", "EgI4AQ%253D%253D")),
        "hdr" => Some(("hdr", "EgPIAQ%253D%253D")),
        "local" => Some(("local", "EgO4AQ%253D%253D")),
        "newest" => Some(("newest", "CAISAhAB")),
        "views" => Some(("views", "CAMSAhAB")),
        "rating" => Some(("rating", "CAESAhAB")),
        _ => None,
    }
}

pub fn build_search_url(term: &str, sp: &str) -> String {
    format!(
        "https://www.youtube.com/results?search_query={}&sp={}",
        urlencoding::encode(term.trim()),
        sp
    )
}

pub async fn fetch(spec: &FetchSpec) -> Result<FeedData> {
    if !spec.bypass_cache {
        if let Some(cached) = read_cache(spec) {
            return Ok(cached);
        }
    }

    let data = fetch_uncached(spec).await?;
    if !data.items.is_empty() {
        write_cache(spec, &data);
    }
    Ok(data)
}

async fn fetch_uncached(spec: &FetchSpec) -> Result<FeedData> {
    let mut command = Command::new("yt-dlp");
    command
        .arg("--dump-single-json")
        .arg("--flat-playlist")
        .arg("--no-warnings")
        .arg("--ignore-errors")
        .arg("--no-colors")
        .arg("--socket-timeout")
        .arg("15")
        .arg("--retries")
        .arg("1")
        .arg("--extractor-retries")
        .arg("1");

    if spec.page > 1 {
        let (start, end) = playlist_window(spec.page, spec.limit);
        command.arg("--playlist-start").arg(start.to_string());
        command.arg("--playlist-end").arg(end.to_string());
    } else {
        command
            .arg("--playlist-end")
            .arg(spec.limit.max(1).to_string());
    }

    if let Some(browser) = &spec.cookies_browser {
        command.arg("--cookies-from-browser").arg(browser);
    }

    command.arg("--").arg(&spec.url).kill_on_drop(true);

    // Paged fetches walk more continuation chunks before their window opens
    // (--playlist-start 151 must extract 150 entries first), so they get
    // twice the runway before the spinner gives up.
    let timeout_secs = if spec.page > 1 { 150 } else { 75 };
    let output = tokio::time::timeout(Duration::from_secs(timeout_secs), command.output())
        .await
        .map_err(|_| anyhow::anyhow!("yt-dlp timed out after {timeout_secs} seconds"))?
        .context("could not run yt-dlp (is it installed and on PATH?)")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.trim().is_empty() {
        let detail = last_error_line(&stderr);
        if spec.requires_auth {
            if spec.cookies_browser.is_none() {
                bail!(
                    "This feed needs YouTube cookies. Set cookies_browser = \"firefox\" \
                     (or another browser) in the Utui config."
                );
            }
            bail!(
                "YouTube returned no items. Make sure you are signed in to that browser{}.",
                detail.map(|line| format!(" ({line})")).unwrap_or_default()
            );
        }
        if !output.status.success() {
            bail!(
                "yt-dlp failed{}",
                detail.map(|line| format!(": {line}")).unwrap_or_default()
            );
        }
        bail!("No results returned by yt-dlp");
    }

    let root: Value = serde_json::from_str(&stdout).context("yt-dlp returned invalid JSON")?;
    let items = extract_items(&root, spec.limit);
    let mut title = root
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(&spec.label)
        .to_string();

    if title == "NA" || title.is_empty() {
        title = spec.label.clone();
    }

    if items.is_empty() {
        if spec.requires_auth && spec.cookies_browser.is_none() {
            bail!(
                "This feed needs YouTube cookies. Set cookies_browser = \"firefox\" \
                 (or another browser) in the Utui config."
            );
        }
        let detail = last_error_line(&stderr);
        if spec.requires_auth {
            bail!(
                "No videos found. Check that the selected browser is signed in to YouTube{}.",
                detail.map(|line| format!(" ({line})")).unwrap_or_default()
            );
        }
    }

    Ok(FeedData {
        title,
        source: spec.url.clone(),
        items,
        fetched_at: now_unix(),
    })
}

fn extract_items(root: &Value, limit: usize) -> Vec<Video> {
    let root_channel = root
        .get("channel")
        .and_then(Value::as_str)
        .or_else(|| root.get("uploader").and_then(Value::as_str));
    let root_channel_url = root
        .get("channel_url")
        .and_then(Value::as_str)
        .or_else(|| root.get("uploader_url").and_then(Value::as_str));

    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![root.clone()]);

    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for entry in entries {
        if entry.is_null() {
            continue;
        }
        if let Some(video) = parse_video(&entry, root_channel, root_channel_url) {
            if seen.insert(video.id.clone()) {
                items.push(video);
            }
        }
        if items.len() >= limit {
            break;
        }
    }
    items
}

fn parse_video(
    value: &Value,
    root_channel: Option<&str>,
    root_channel_url: Option<&str>,
) -> Option<Video> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("url")
                .and_then(Value::as_str)
                .and_then(video_id_from_url)
        })?;

    let title = value
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| value.get("fulltitle").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }

    let url = value
        .get("webpage_url")
        .and_then(Value::as_str)
        .or_else(|| value.get("url").and_then(Value::as_str))
        .filter(|url| url.starts_with("http"))
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}"));

    let channel = value
        .get("channel")
        .and_then(Value::as_str)
        .or_else(|| value.get("uploader").and_then(Value::as_str))
        .or(root_channel)
        .unwrap_or("Unknown channel")
        .to_string();

    let channel_url = value
        .get("channel_url")
        .and_then(Value::as_str)
        .or_else(|| value.get("uploader_url").and_then(Value::as_str))
        .or(root_channel_url)
        .map(normalize_url);

    let duration = value.get("duration").and_then(|duration| {
        duration
            .as_u64()
            .or_else(|| duration.as_f64().map(|v| v as u64))
    });
    let view_count = value
        .get("view_count")
        .and_then(|views| views.as_u64().or_else(|| views.as_f64().map(|v| v as u64)));

    let upload_date = value
        .get("upload_date")
        .and_then(Value::as_str)
        .filter(|date| date.len() == 8)
        .map(ToString::to_string);
    let timestamp = value
        .get("timestamp")
        .or_else(|| value.get("release_timestamp"))
        .and_then(Value::as_i64)
        .filter(|timestamp| *timestamp > 0);

    let thumbnail = best_thumbnail(value);
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .map(ToString::to_string);

    let live_status = value
        .get("live_status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let live = matches!(
        live_status,
        "is_live" | "is_upcoming" | "post_live" | "was_live"
    );

    Some(Video {
        id,
        title,
        url,
        channel,
        channel_url,
        duration,
        view_count,
        timestamp,
        upload_date,
        thumbnail,
        description,
        live,
    })
}

fn best_thumbnail(value: &Value) -> Option<String> {
    if let Some(thumbnail) = value.get("thumbnail").and_then(Value::as_str) {
        if thumbnail.starts_with("http") {
            return Some(thumbnail.to_string());
        }
    }

    value
        .get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|thumbnails| {
            thumbnails
                .iter()
                .filter_map(|thumbnail| {
                    let url = thumbnail.get("url").and_then(Value::as_str)?;
                    if !url.starts_with("http") {
                        return None;
                    }
                    let width = thumbnail.get("width").and_then(Value::as_u64).unwrap_or(0);
                    let height = thumbnail.get("height").and_then(Value::as_u64).unwrap_or(0);
                    Some((width.saturating_mul(height), url.to_string()))
                })
                .max_by_key(|(area, _)| *area)
                .map(|(_, url)| url)
        })
}

fn video_id_from_url(url: &str) -> Option<String> {
    if let Some(query) = url.split('?').nth(1) {
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("v=") {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    if let Some(rest) = url.split("youtu.be/").nth(1) {
        let id: String = rest
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
            })
            .collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    None
}

fn normalize_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("//") {
        format!("https:{url}")
    } else if url.starts_with('/') {
        format!("https://www.youtube.com{url}")
    } else {
        format!("https://{url}")
    }
}

fn last_error_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            if line.chars().count() > 220 {
                format!("{}…", line.chars().take(220).collect::<String>())
            } else {
                line.to_string()
            }
        })
}

fn cache_path(spec: &FetchSpec) -> Option<PathBuf> {
    let cache_dir = spec.cache_dir.as_ref()?;
    let key = format!(
        "{}|{}|{}|{}|v3",
        spec.url,
        spec.limit,
        spec.page,
        spec.cookies_browser.as_deref().unwrap_or("")
    );
    Some(
        cache_dir
            .join("feeds")
            .join(format!("{:016x}.json", fnv1a64(key.as_bytes()))),
    )
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedFeed {
    saved_at: i64,
    data: FeedData,
}

fn read_cache(spec: &FetchSpec) -> Option<FeedData> {
    if spec.cache_ttl_secs == 0 {
        return None;
    }
    let path = cache_path(spec)?;
    let text = std::fs::read_to_string(path).ok()?;
    let cached: CachedFeed = serde_json::from_str(&text).ok()?;
    if now_unix().saturating_sub(cached.saved_at) <= spec.cache_ttl_secs as i64 {
        Some(cached.data)
    } else {
        None
    }
}

/// Feed cache files are tiny but unbounded in number (one per URL window);
/// this cap keeps `~/.cache/utui/feeds` a small directory — the oldest
/// entries fall off on every write past it.
const MAX_FEED_CACHE_FILES: usize = 64;

fn write_cache(spec: &FetchSpec, data: &FeedData) {
    let Some(path) = cache_path(spec) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(body) = serde_json::to_string(&CachedFeed {
        saved_at: now_unix(),
        data: data.clone(),
    }) {
        let _ = write_atomic(&path, &body);
        crate::util::prune_cache_dir(parent, MAX_FEED_CACHE_FILES);
    }
}

pub fn clear_cache(cache_dir: &Path) -> Result<()> {
    let path = cache_dir.join("feeds");
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("could not clear {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_search_filters() {
        let plan = parse_search("rust programming :today");
        assert_eq!(plan.term, "rust programming");
        assert_eq!(plan.filter.as_deref(), Some("today"));
        assert_eq!(plan.sp, "EgIIAg%253D%253D");

        let plan = parse_search(":4k synthwave");
        assert_eq!(plan.term, "synthwave");
        assert_eq!(plan.filter.as_deref(), Some("4k"));

        let plan = parse_search("no filter here");
        assert_eq!(plan.term, "no filter here");
        assert_eq!(plan.filter, None);
        assert_eq!(plan.sp, DEFAULT_SEARCH_SP);
    }

    #[test]
    fn leaves_only_filter_empty() {
        let plan = parse_search(":today");
        assert!(plan.term.is_empty());
        assert_eq!(plan.filter.as_deref(), Some("today"));
    }

    #[test]
    fn builds_encoded_search_url() {
        let url = build_search_url("rust & lofi", DEFAULT_SEARCH_SP);
        assert!(url.contains("rust%20%26%20lofi"));
        assert!(url.contains(DEFAULT_SEARCH_SP));
    }

    #[test]
    fn extracts_flat_playlist() {
        let root = json!({
            "title": "Demo",
            "entries": [
                {
                    "id": "abc123",
                    "title": "A video",
                    "url": "https://www.youtube.com/watch?v=abc123",
                    "duration": 62.0,
                    "view_count": 1200,
                    "channel": "A channel",
                    "channel_url": "/channel/UC123",
                    "thumbnails": [
                        {"url": "https://example.com/small.jpg", "width": 120, "height": 90},
                        {"url": "https://example.com/large.jpg", "width": 1280, "height": 720}
                    ]
                },
                {"id": "abc123", "title": "duplicate"},
                {"id": "def456", "title": ""}
            ]
        });
        let items = extract_items(&root, 50);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].duration, Some(62));
        assert_eq!(items[0].view_count, Some(1200));
        assert_eq!(
            items[0].channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC123")
        );
        assert_eq!(
            items[0].thumbnail.as_deref(),
            Some("https://example.com/large.jpg")
        );
    }

    #[test]
    fn fills_channel_from_playlist_root() {
        let root = json!({
            "title": "Channel",
            "channel": "Root Channel",
            "channel_url": "https://www.youtube.com/channel/UCROOT",
            "entries": [{"id": "x", "title": "Video"}]
        });
        let items = extract_items(&root, 10);
        assert_eq!(items[0].channel, "Root Channel");
        assert_eq!(
            items[0].channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UCROOT")
        );
    }

    #[test]
    fn page_windows_are_contiguous_yt_x_style() {
        assert_eq!(playlist_window(1, 30), (1, 30));
        assert_eq!(playlist_window(2, 30), (31, 60));
        assert_eq!(playlist_window(3, 30), (61, 90));
        // A degenerate limit never yields an inverted window.
        assert_eq!(playlist_window(2, 0), (2, 2));
    }

    #[test]
    fn cache_key_differs_per_page() {
        let mut spec = FetchSpec::new(1, "https://www.youtube.com/playlist?list=WL", "Watch Later");
        spec.limit = 30;
        spec.cache_dir = Some(std::path::PathBuf::from("/tmp/utui-cache-test"));
        let page1 = cache_path(&spec);
        spec.page = 2;
        let page2 = cache_path(&spec);
        assert_ne!(page1, page2, "pages must not share a cache file");
        spec.page = 1;
        assert_eq!(page1, cache_path(&spec), "page 1 key is stable");
    }
}
