use crate::config::{Config, Paths};
use crate::store::{RememberedFeed, Store};
use crate::sync::SyncAction;
use crate::theme::Theme;
use crate::util::{copy_to_clipboard, now_unix};
use crate::yt::{self, FeedData, FetchSpec, Video};
use anyhow::{bail, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::DynamicImage;
use ratatui::widgets::TableState;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

const TOAST_LIFETIME: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Home,
    Browse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Search,
    Remote,
    Channel,
    Playlist,
    Saved,
    History,
    /// The play queue: a local list of everything queued with X/Z.
    Queue,
}

impl FeedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "SEARCH",
            Self::Remote => "REMOTE",
            Self::Channel => "CHANNEL",
            Self::Playlist => "PLAYLIST",
            Self::Saved => "LOCAL COLLECTION",
            Self::History => "WATCHED LOCALLY",
            Self::Queue => "PLAY QUEUE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Feed {
    pub title: String,
    pub source: String,
    pub kind: FeedKind,
    pub subtitle: String,
    pub items: Vec<Video>,
    pub notice: Option<String>,
    pub auth: bool,
    pub fetched_at: i64,
}

impl Feed {
    pub fn empty() -> Self {
        Self {
            title: String::new(),
            source: String::new(),
            kind: FeedKind::Remote,
            subtitle: String::new(),
            items: Vec::new(),
            notice: None,
            auth: false,
            fetched_at: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    created: Instant,
}

impl Toast {
    pub fn is_visible(&self, now: Instant) -> bool {
        now.duration_since(self.created) < TOAST_LIFETIME
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(character) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match character.to_ascii_lowercase() {
                        'u' => {
                            self.value.clear();
                            self.cursor = 0;
                            true
                        }
                        'w' => {
                            self.delete_word();
                            true
                        }
                        'a' => {
                            self.cursor = 0;
                            true
                        }
                        'e' => {
                            self.cursor = self.char_count();
                            true
                        }
                        _ => false,
                    }
                } else {
                    self.insert(character);
                    true
                }
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.char_count());
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.char_count();
                true
            }
            _ => false,
        }
    }

    pub fn insert_str(&mut self, value: &str) {
        for character in value.chars() {
            self.insert(character);
        }
    }

    pub fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    pub fn cursor_byte(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len())
    }

    pub fn cursor_width(&self) -> usize {
        crate::util::display_width(&self.value[..self.cursor_byte()])
    }

    fn insert(&mut self, character: char) {
        let byte = self.cursor_byte();
        self.value.insert(byte, character);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self.cursor - 1;
        let start = self
            .value
            .char_indices()
            .nth(previous)
            .map(|(byte, _)| byte)
            .unwrap_or(0);
        let end = self.cursor_byte();
        self.value.replace_range(start..end, "");
        self.cursor = previous;
    }

    fn delete(&mut self) {
        if self.cursor >= self.char_count() {
            return;
        }
        let start = self.cursor_byte();
        let end = self
            .value
            .char_indices()
            .nth(self.cursor + 1)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len());
        self.value.replace_range(start..end, "");
    }

    fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.value.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let start_byte = self
            .value
            .char_indices()
            .nth(start)
            .map(|(byte, _)| byte)
            .unwrap_or(0);
        let end_byte = self.cursor_byte();
        self.value.replace_range(start_byte..end_byte, "");
        self.cursor = start;
    }
}

#[derive(Debug)]
pub enum Overlay {
    Search(TextInput),
    Quality { index: usize },
    Help,
}

/// The `d` quality picker. Each entry is (label, yt-dlp `height<=` cap);
/// a cap of 0 means no cap, i.e. the best format.
pub const QUALITIES: &[(&str, u32)] = &[
    ("best", 0),
    ("2160p", 2160),
    ("1440p", 1440),
    ("1080p", 1080),
    ("720p", 720),
    ("480p", 480),
    ("360p", 360),
    ("250p", 250),
    ("160p", 160),
];

#[derive(Debug)]
pub enum Effect {
    /// Hand one video — or a whole queue tail — to the player as a single
    /// mpv invocation; multiple URLs play back to back as one playlist.
    Play {
        videos: Vec<Video>,
        audio: bool,
    },
    Download {
        job_id: u64,
        video: Video,
        audio: bool,
        /// yt-dlp `height<=` cap from the quality picker (None = best).
        height: Option<u32>,
    },
    Copy(String),
    OpenUrl(String),
    Sync {
        video: Video,
        action: SyncAction,
    },
}

#[derive(Debug)]
pub enum Msg {
    Feed {
        id: u64,
        result: Result<FeedData, String>,
    },
    DownloadProgress {
        id: u64,
        percent: u8,
    },
    DownloadDone {
        id: u64,
        ok: bool,
        message: String,
    },
    Thumbnail {
        id: String,
        result: Result<DynamicImage, String>,
    },
    SyncDone {
        video_id: String,
        action: SyncAction,
        ok: bool,
        message: String,
    },
    /// Signed-in account name for the header (fetched in the background).
    AccountName(Option<String>),
}

/// A thumbnail fetch request queued by the app and dispatched by the runtime.
#[derive(Debug, Clone)]
pub struct ThumbSpec {
    pub id: String,
    pub url: String,
    pub path: PathBuf,
}

/// Lifecycle of the thumbnail shown in the details pane.
pub enum ThumbnailState {
    Idle,
    Loading {
        id: String,
    },
    Ready {
        id: String,
        protocol: StatefulProtocol,
    },
    Failed {
        id: String,
    },
}

impl ThumbnailState {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Loading { id } | Self::Ready { id, .. } | Self::Failed { id } => Some(id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadView {
    pub job_id: u64,
    pub title: String,
    pub percent: u8,
}

#[derive(Debug, Clone, Copy)]
pub enum HomeAction {
    Remote {
        title: &'static str,
        url: &'static str,
        auth: bool,
    },
    Saved,
    History,
}

pub struct HomeItem {
    pub key: &'static str,
    pub title: &'static str,
    pub action: HomeAction,
    /// Not shown in the horizontal menu bar, still reachable by its key
    /// (and by last-feed restore).
    pub hidden: bool,
}

pub fn visible_home_items() -> impl Iterator<Item = &'static HomeItem> {
    HOME_ITEMS.iter().filter(|item| !item.hidden)
}

pub const HOME_ITEMS: &[HomeItem] = &[
    // The YouTube home page itself — the personalized "My Feeds" view.
    // Cookie-gated like Watch Later and Liked: YouTube serves logged-out
    // visitors an empty home page, so it syncs through cookies_browser.
    HomeItem {
        key: "F",
        title: "My Feeds",
        action: HomeAction::Remote {
            title: "My Feeds",
            url: "https://www.youtube.com/",
            auth: true,
        },
        hidden: false,
    },
    HomeItem {
        key: "w",
        title: "Watch Later",
        action: HomeAction::Remote {
            title: "Watch Later",
            url: "https://www.youtube.com/playlist?list=WL",
            auth: true,
        },
        hidden: false,
    },
    HomeItem {
        key: "l",
        title: "Liked videos",
        action: HomeAction::Remote {
            title: "Liked videos",
            url: "https://www.youtube.com/playlist?list=LL",
            auth: true,
        },
        hidden: false,
    },
    HomeItem {
        key: "S",
        title: "Saved videos",
        action: HomeAction::Saved,
        hidden: false,
    },
    HomeItem {
        key: "H",
        title: "History",
        action: HomeAction::History,
        hidden: false,
    },
    // Cookie-path subscriptions stay a shortcut (`f`) and are restorable,
    // but are not part of the horizontal menu bar.
    HomeItem {
        key: "f",
        title: "Subscriptions",
        action: HomeAction::Remote {
            title: "Subscriptions",
            url: "https://www.youtube.com/feed/subscriptions",
            auth: true,
        },
        hidden: true,
    },
];

pub struct App {
    pub paths: Paths,
    pub config: Config,
    pub store: Store,
    pub view: View,
    pub feed: Feed,
    pub stack: Vec<Feed>,
    pub overlay: Option<Overlay>,
    pub table_state: TableState,
    pub home_index: usize,
    pub loading: bool,
    pub loading_label: String,
    pub download: Option<DownloadView>,
    pub toast: Option<Toast>,
    pub quit: bool,
    pub last_search: Option<String>,
    /// Home-menu key of the feed currently loading, for last-feed memory.
    home_origin: Option<&'static str>,
    pub picker: Option<Picker>,
    pub thumbnail: ThumbnailState,
    fetch_id: u64,
    download_id: u64,
    pending_fetch: Option<FetchSpec>,
    pending_effect: Option<Effect>,
    pending_thumbnail: Option<ThumbSpec>,
    started: Instant,
    /// Wallpaper-tool palette (pywal / wallust / matugen), reloaded live by
    /// `poll_theme` when the tool rewrites its colours.
    pub theme: Theme,
    /// The palette file the theme came from, plus the mtime we last read —
    /// together these let `poll_theme` notice a wallpaper change with a
    /// single `stat()` in the steady state.
    theme_src: Option<PathBuf>,
    theme_mtime: Option<SystemTime>,
    theme_poll_at: Instant,
    /// Signed-in account name, fetched in the background for the header.
    pub account_name: Option<String>,
    /// 1-based page window into the current feed (yt-x style paging: each
    /// page re-queries the same list with a shifted --playlist-start window).
    pub feed_page: usize,
    /// Set when the last fetch came back short of a full page — there is
    /// nothing beyond `feed_page`.
    pub feed_last_page: bool,
}

impl App {
    pub fn new(paths: Paths, config: Config, store: Store) -> Self {
        // The detected palette, plus the file it came from and that file's
        // mtime — together these let `poll_theme` notice a wallpaper change
        // and repaint without re-reading the palette every tick.
        let (theme, theme_src) = match Theme::load_with_source() {
            Some((theme, path)) => (theme, Some(path)),
            None => (Theme::default(), None),
        };
        let theme_mtime = theme_src
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let mut app = Self {
            paths,
            config,
            store,
            view: View::Home,
            feed: Feed::empty(),
            stack: Vec::new(),
            overlay: None,
            table_state: TableState::default(),
            home_index: 0,
            loading: false,
            loading_label: String::new(),
            download: None,
            toast: None,
            quit: false,
            last_search: None,
            home_origin: None,
            picker: None,
            thumbnail: ThumbnailState::Idle,
            fetch_id: 0,
            download_id: 0,
            pending_fetch: None,
            pending_effect: None,
            pending_thumbnail: None,
            started: Instant::now(),
            theme,
            theme_src,
            theme_mtime,
            theme_poll_at: Instant::now(),
            account_name: None,
            feed_page: 1,
            feed_last_page: false,
        };
        app.restore_last_feed();
        app
    }

    pub fn take_fetch(&mut self) -> Option<FetchSpec> {
        self.pending_fetch.take()
    }

    /// Reopen the feed that was on screen when the last run ended, so Utui
    /// always starts inside a feed instead of on the home menu.
    pub fn restore_last_feed(&mut self) {
        let Some(last) = self.store.last_feed.clone() else {
            return;
        };
        if let Some(key) = last.home_key {
            self.open_home_item_by_key(&key);
        } else if let Some(source) = last.source {
            let title = last.title.clone().unwrap_or_else(|| "Feed".to_string());
            self.request_feed(
                source,
                title,
                String::new(),
                FeedKind::Remote,
                last.auth,
                false,
                false,
                View::Home,
            );
        }
    }

    /// Persists the last-viewed feed so the next launch reopens it.
    fn remember_last_feed(&mut self) {
        let remembered = if let Some(key) = self.home_origin {
            RememberedFeed {
                home_key: Some(key.to_string()),
                source: None,
                title: None,
                auth: self.feed.auth,
            }
        } else if self.feed.items.is_empty() {
            return;
        } else {
            RememberedFeed {
                home_key: None,
                source: if self.feed.source.starts_with("utui://") {
                    None
                } else {
                    Some(self.feed.source.clone())
                },
                title: Some(self.feed.title.clone()),
                auth: self.feed.auth,
            }
        };
        self.home_origin = None;
        self.store.last_feed = Some(remembered);
        let _ = self.persist();
    }

    pub fn take_effect(&mut self) -> Option<Effect> {
        self.pending_effect.take()
    }

    pub fn take_thumbnail(&mut self) -> Option<ThumbSpec> {
        self.pending_thumbnail.take()
    }

    /// Install the terminal image picker detected at startup.
    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = Some(picker);
    }

    /// Drop any rendered thumbnail so it is rebuilt on the next frame.
    pub fn clear_thumbnail(&mut self) {
        self.thumbnail = ThumbnailState::Idle;
    }

    /// Make sure the details pane is loading the thumbnail for the current
    /// selection. Idempotent, so it is safe to call every frame.
    pub fn sync_thumbnail(&mut self) {
        let Some(video) = self.selected_video() else {
            self.thumbnail = ThumbnailState::Idle;
            return;
        };
        let id = video.id.clone();
        if self.thumbnail.id() == Some(id.as_str()) {
            return;
        }
        let Some(url) = video.thumbnail.clone() else {
            self.thumbnail = ThumbnailState::Failed { id };
            return;
        };
        self.thumbnail = ThumbnailState::Loading { id: id.clone() };
        self.pending_thumbnail = Some(ThumbSpec {
            id: id.clone(),
            url,
            path: self.paths.thumbnail_file(&id),
        });
    }

    pub fn spinner_frame(&self, now: Instant) -> &'static str {
        const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let index = (now.duration_since(self.started).as_millis() / 80) as usize % FRAMES.len();
        FRAMES[index]
    }

    pub fn active_toast(&self, now: Instant) -> Option<&Toast> {
        self.toast.as_ref().filter(|toast| toast.is_visible(now))
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state
            .selected()
            .filter(|index| *index < self.feed.items.len())
    }

    pub fn selected_video(&self) -> Option<&Video> {
        self.selected_index()
            .and_then(|index| self.feed.items.get(index))
    }

    pub fn can_go_back(&self) -> bool {
        !self.stack.is_empty()
    }

    pub fn start_search(&mut self, query: &str) -> Result<()> {
        let resolved = self.store.recall_search(query);
        let plan = yt::parse_search(&resolved);
        if plan.term.trim().is_empty() {
            bail!("Add a search term after the filter");
        }

        self.store.add_search(&resolved);
        self.last_search = Some(resolved);
        self.stack.clear();
        let subtitle = plan
            .filter
            .as_ref()
            .map(|filter| format!("FILTER  :{filter}  ·  {} requested", self.config.results))
            .unwrap_or_else(|| format!("{} requested", self.config.results));
        let title = format!("Search · {}", plan.term);
        let url = yt::build_search_url(&plan.term, &plan.sp);
        let _ = self.persist();
        self.home_origin = None;
        self.request_feed(
            url,
            title,
            subtitle,
            FeedKind::Search,
            false,
            false,
            false,
            self.view,
        );
        Ok(())
    }

    pub fn start_url(&mut self, input: &str) -> Result<()> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("Paste a URL first");
        }
        if trimmed.chars().any(char::is_whitespace) && !trimmed.starts_with("ytsearch:") {
            bail!("That looks like a search query. Press / to search instead");
        }
        let url = if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("ytsearch:")
        {
            trimmed.to_string()
        } else {
            format!("https://{trimmed}")
        };

        let push = self.view == View::Browse && !self.feed.items.is_empty();
        let kind = if url.contains("list=") {
            FeedKind::Playlist
        } else if url.contains("/channel/") || url.contains("/@") {
            FeedKind::Channel
        } else {
            FeedKind::Remote
        };
        self.home_origin = None;
        self.request_feed(
            url,
            "Opening URL".to_string(),
            kind.label().to_string(),
            kind,
            false,
            push,
            false,
            self.view,
        );
        Ok(())
    }

    /// True when search-prompt input should be treated as a URL instead of a
    /// query: explicit schemes, `ytsearch:`, or a single token containing a dot.
    pub fn looks_like_url(input: &str) -> bool {
        let lower = input.to_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("ytsearch:")
        {
            return true;
        }
        !input.chars().any(char::is_whitespace) && input.contains('.')
    }

    pub fn open_visible_home_item(&mut self, index: usize) {
        let Some(item) = visible_home_items().nth(index) else {
            return;
        };
        self.open_home_entry(item);
    }

    fn open_home_entry(&mut self, item: &'static HomeItem) {
        // Highlight the bar entry whose feed is being opened, so the bar reads
        // as "where you are". Hidden items (f) are not part of the bar.
        if let Some(index) = visible_home_items().position(|other| other.key == item.key) {
            self.home_index = index;
        }
        // Feed jumps land on Home (the bar lives there). From Browse the feed
        // you were on is pushed so `b` can return to it.
        if self.view == View::Browse && !self.feed.items.is_empty() {
            self.stack.push(self.feed.clone());
        }
        self.view = View::Home;
        match item.action {
            HomeAction::Remote { title, url, auth } => {
                self.home_origin = Some(item.key);
                self.request_feed(
                    url.to_string(),
                    title.to_string(),
                    "YOUTUBE · COOKIES".to_string(),
                    FeedKind::Remote,
                    auth,
                    false,
                    false,
                    View::Home,
                );
            }
            HomeAction::Saved => {
                self.home_origin = Some(item.key);
                self.show_saved(false);
            }
            HomeAction::History => {
                self.home_origin = Some(item.key);
                self.show_history(false);
            }
        }
    }

    /// Open a home entry by its key letter, so shortcuts never depend on list order.
    fn open_home_item_by_key(&mut self, key: &str) {
        if let Some(item) = HOME_ITEMS.iter().find(|item| item.key == key) {
            self.open_home_entry(item);
        }
    }

    pub fn show_saved(&mut self, push: bool) {
        if push && !self.feed.items.is_empty() {
            self.stack.push(self.feed.clone());
        }
        let items = self.store.saved.clone();
        self.feed = Feed {
            title: "Saved videos".to_string(),
            source: "utui://saved".to_string(),
            kind: FeedKind::Saved,
            subtitle: format!("{} VIDEOS", items.len()),
            notice: if items.is_empty() {
                Some("Nothing saved yet. Press `a` on a result to keep it here.".to_string())
            } else {
                None
            },
            items,
            auth: false,
            fetched_at: now_unix(),
        };
        self.finish_local_feed();
    }

    pub fn show_history(&mut self, push: bool) {
        if push && !self.feed.items.is_empty() {
            self.stack.push(self.feed.clone());
        }
        let items: Vec<Video> = self
            .store
            .history
            .iter()
            .map(|entry| entry.video.clone())
            .collect();
        self.feed = Feed {
            title: "History".to_string(),
            source: "utui://history".to_string(),
            kind: FeedKind::History,
            subtitle: format!("{} VIDEOS", items.len()),
            notice: if items.is_empty() {
                Some("Nothing watched yet. Play something and it will appear here.".to_string())
            } else {
                None
            },
            items,
            auth: false,
            fetched_at: now_unix(),
        };
        self.finish_local_feed();
    }

    /// The play queue as a local list, in the order mpv will play the videos
    /// back to back. `push` remembers the feed you came from so `b` steps out.
    pub fn show_queue(&mut self, push: bool) {
        if push && self.feed.kind != FeedKind::Queue && !self.feed.items.is_empty() {
            self.stack.push(self.feed.clone());
        }
        let items = self.store.queue.clone();
        self.feed = Feed {
            title: "Queue".to_string(),
            source: "utui://queue".to_string(),
            kind: FeedKind::Queue,
            subtitle: format!("{} QUEUED", items.len()),
            notice: if items.is_empty() {
                Some("Nothing queued. Press `X` on a result to play it next, `Z` to add it at the end.".to_string())
            } else {
                None
            },
            items,
            auth: false,
            fetched_at: now_unix(),
        };
        self.finish_local_feed();
    }

    /// `z`: open the play queue page, remembering the feed you were on.
    pub fn open_queue(&mut self) {
        if self.feed.kind == FeedKind::Queue {
            return;
        }
        self.show_queue(true);
    }

    /// Keep the open queue page in step with the store after an edit.
    fn sync_queue_feed(&mut self) {
        if self.feed.kind != FeedKind::Queue {
            return;
        }
        self.feed.items = self.store.queue.clone();
        self.feed.subtitle = format!("{} QUEUED", self.feed.items.len());
        if self.feed.items.is_empty() {
            self.feed.notice = Some(
                "Nothing queued. Press `X` on a result to play it next, `Z` to add it at the end."
                    .to_string(),
            );
            self.table_state.select(None);
        } else {
            self.feed.notice = None;
            let last = self.feed.items.len() - 1;
            let selected = self.table_state.selected().unwrap_or(0).min(last);
            self.table_state.select(Some(selected));
        }
    }

    /// `X`: queue the highlighted video to play next (mpv's playlist front).
    /// On the queue page it promotes the selected row instead.
    pub fn queue_selected_next(&mut self) {
        if self.feed.kind == FeedKind::Queue {
            self.queue_promote_selected();
            return;
        }
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        self.store.queue_insert_next(&video);
        let _ = self.persist();
        self.sync_queue_feed();
        self.toast_success(format!("Playing next: {}", video.title));
    }

    /// `Z`: append the highlighted video to the end of the queue. On the
    /// queue page it moves the selected row to the end instead.
    pub fn queue_selected_append(&mut self) {
        if self.feed.kind == FeedKind::Queue {
            self.queue_demote_selected();
            return;
        }
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        self.store.queue_append(&video);
        let _ = self.persist();
        self.sync_queue_feed();
        self.toast_success(format!("Added to the end of the queue: {}", video.title));
    }

    /// On the queue page, `X` moves the selected row to the front.
    fn queue_promote_selected(&mut self) {
        let Some(index) = self.table_state.selected() else {
            return;
        };
        if index == 0 || index >= self.store.queue.len() {
            return;
        }
        let video = self.store.queue.remove(index);
        self.store.queue.insert(0, video);
        let _ = self.persist();
        self.sync_queue_feed();
        self.table_state.select(Some(0));
        self.toast_info("Moved to the front of the queue");
    }

    /// On the queue page, `Z` moves the selected row to the end.
    fn queue_demote_selected(&mut self) {
        let Some(index) = self.table_state.selected() else {
            return;
        };
        let len = self.store.queue.len();
        if len < 2 || index + 1 >= len {
            return;
        }
        let video = self.store.queue.remove(index);
        self.store.queue.push(video);
        let _ = self.persist();
        self.sync_queue_feed();
        self.table_state.select(Some(len - 1));
        self.toast_info("Moved to the end of the queue");
    }

    /// `J`/`K` on the queue page: move the selected video down/up.
    pub fn queue_move_selected(&mut self, down: bool) {
        let Some(index) = self.table_state.selected() else {
            return;
        };
        let len = self.store.queue.len();
        if len < 2 {
            return;
        }
        let swap = if down { index + 1 < len } else { index > 0 };
        if !swap {
            return;
        }
        let other = if down { index + 1 } else { index - 1 };
        self.store.queue.swap(index, other);
        let _ = self.persist();
        self.sync_queue_feed();
        self.table_state.select(Some(other));
    }

    /// `d` on the queue page: drop the selected video.
    pub fn queue_remove_selected(&mut self) {
        let Some(index) = self.table_state.selected() else {
            return;
        };
        if index >= self.store.queue.len() {
            return;
        }
        let removed = self.store.queue.remove(index);
        let _ = self.persist();
        self.sync_queue_feed();
        self.toast_info(format!("Removed from the queue: {}", removed.title));
    }

    /// Enter/Space on the queue page: hand mpv the queue from the selected
    /// video on — the whole playlist goes over in one invocation.
    fn play_selected_queue(&mut self, audio: bool) {
        let Some(start) = self.table_state.selected() else {
            return;
        };
        let videos: Vec<Video> = self.store.queue.iter().skip(start).cloned().collect();
        if videos.is_empty() {
            return;
        }
        // History records only the video playback actually starts with.
        if let Some(first) = videos.first() {
            self.record_watch(first);
        }
        self.pending_effect = Some(Effect::Play { videos, audio });
    }

    fn finish_local_feed(&mut self) {
        self.fetch_id = self.fetch_id.wrapping_add(1);
        self.pending_fetch = None;
        self.loading = false;
        // Local lists hold everything at once; a queue opened over a paged
        // feed must not inherit its window.
        self.feed_page = 1;
        self.feed_last_page = false;
        // Keep the caller's view: the bar opens these feeds in place on Home,
        // while a refresh from Browse should stay in Browse.
        self.overlay = None;
        self.reset_selection();
        match self.feed.kind {
            FeedKind::Saved => self.remember_home_feed("S"),
            FeedKind::History => self.remember_home_feed("H"),
            // The queue is not a launch feed; `z` reopens it in one key.
            FeedKind::Queue => {}
            _ => self.remember_last_feed(),
        }
    }

    /// Local collection feeds are remembered by their home-menu shortcut key.
    fn remember_home_feed(&mut self, key: &'static str) {
        self.store.last_feed = Some(RememberedFeed {
            home_key: Some(key.to_string()),
            source: None,
            title: Some(self.feed.title.clone()),
            auth: false,
        });
        self.home_origin = None;
        let _ = self.persist();
    }

    #[allow(clippy::too_many_arguments)]
    fn request_feed(
        &mut self,
        url: String,
        title: String,
        subtitle: String,
        kind: FeedKind,
        requires_auth: bool,
        push: bool,
        bypass_cache: bool,
        view: View,
    ) {
        if requires_auth && self.config.cookies_browser.is_none() {
            self.feed_page = 1;
            self.feed_last_page = false;
            self.feed = Feed {
                title,
                source: url,
                kind,
                subtitle,
                items: Vec::new(),
                notice: Some(
                    "This feed needs YouTube cookies. Set `cookies_browser = \"firefox\"` \
                     (or another browser) in config.toml, then press `R`."
                        .to_string(),
                ),
                auth: true,
                fetched_at: 0,
            };
            self.view = view;
            self.overlay = None;
            self.loading = false;
            self.fetch_id = self.fetch_id.wrapping_add(1);
            self.pending_fetch = None;
            self.reset_selection();
            self.toast_info("Cookie-backed feed: configure cookies_browser first");
            return;
        }

        if push && !self.feed.items.is_empty() {
            self.stack.push(self.feed.clone());
        }

        self.fetch_id = self.fetch_id.wrapping_add(1);
        let id = self.fetch_id;
        self.loading = true;
        // Every navigation starts on the first page; paging and refresh keep
        // the page they are on through `refetch_page`.
        self.feed_page = 1;
        self.feed_last_page = false;
        self.loading_label = match kind {
            FeedKind::Search => "Searching YouTube".to_string(),
            FeedKind::Channel => "Loading channel".to_string(),
            FeedKind::Playlist => "Loading playlist".to_string(),
            _ => "Fetching with yt-dlp".to_string(),
        };
        self.feed = Feed {
            title,
            source: url.clone(),
            kind,
            subtitle,
            items: Vec::new(),
            notice: None,
            auth: requires_auth,
            fetched_at: 0,
        };
        self.view = view;
        self.overlay = None;
        self.reset_selection();

        let mut spec = FetchSpec::new(id, url, self.feed.title.clone());
        spec.limit = self.config.results;
        spec.cookies_browser = self.config.cookies_browser.clone();
        spec.requires_auth = requires_auth;
        spec.cache_dir = Some(self.paths.feed_cache_dir());
        spec.bypass_cache = bypass_cache;
        self.pending_fetch = Some(spec);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.quit = true;
            return;
        }

        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                return;
            }
            KeyCode::Char('/') => {
                self.open_search_overlay();
                return;
            }
            KeyCode::Char('q') => {
                self.quit = true;
                return;
            }
            _ => {}
        }

        self.handle_list_key(key);
    }

    /// One key map for both list views (Home and Browse), aligned with the
    /// ytmtui bindings: Enter/Space play, `m` opens the action menu, `w`/`l`/`s`
    /// toggle Watch Later / like / save, `d` picks a quality to download,
    /// `1`-`5` jump to the feeds. The play queue adds `z`/`X`/`Z` everywhere
    /// and J/K/d/Enter on the queue page itself.
    fn handle_list_key(&mut self, key: KeyEvent) {
        // The queue page owns a few extra keys: J/K reorder the rows, `d`
        // removes one (ytmtui parity), and Enter/Space play the queue from
        // the highlighted video rather than a single one.
        if self.feed.kind == FeedKind::Queue {
            match key.code {
                KeyCode::Char('J') => return self.queue_move_selected(true),
                KeyCode::Char('K') => return self.queue_move_selected(false),
                KeyCode::Char('d') => return self.queue_remove_selected(),
                KeyCode::Enter | KeyCode::Char(' ') => return self.play_selected_queue(false),
                KeyCode::Char('A') => return self.play_selected_queue(true),
                _ => {}
            }
        }
        match key.code {
            // Navigation: arrows, j/k, n/p (next/previous), g/G, paging.
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h' | 'p') => {
                self.move_selection(-1);
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'n') | KeyCode::Tab => {
                self.move_selection(1);
            }
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home | KeyCode::Char('g') => self.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.select_last(),

            // Play, ytmtui style: Enter or Space. On an empty Home, Enter
            // opens the highlighted bar cell instead.
            KeyCode::Enter => {
                if !self.feed.items.is_empty() {
                    self.play_selected(false);
                } else if self.view == View::Home {
                    self.open_visible_home_item(self.home_index);
                }
            }
            KeyCode::Char(' ') => self.play_selected(false),

            // Feed jumps by digit (the home bar shows them, 1. My Feeds
            // first); the leftover letters F/S/H/f still work. w and l are
            // toggles now, not jumps.
            KeyCode::Char(letter @ ('f' | 'F' | 'S' | 'H')) => {
                self.open_home_item_by_key(&letter.to_string())
            }
            KeyCode::Char(digit @ '1'..='5') => {
                let index = digit as usize - '1' as usize;
                if let Some(item) = HOME_ITEMS.get(index) {
                    self.open_home_entry(item);
                }
            }

            // Library toggles on the highlighted video: w Watch Later (press
            // again to unsave), l like/unlike, s save/unsave (a is an alias;
            // uppercase W/L match the old keys), r removes from Saved.
            KeyCode::Char('w' | 'W') => self.toggle_watch_later_selected(),
            KeyCode::Char('l' | 'L') => self.toggle_like_selected(),
            KeyCode::Char('a' | 's') => self.toggle_selected_saved(),
            KeyCode::Char('r') => self.remove_selected_saved(),

            // The rest of the video actions.
            KeyCode::Char('A') => self.play_selected(true),
            KeyCode::Char('d') => self.open_quality(),
            KeyCode::Char('D') => self.download_selected(true, None),
            KeyCode::Char('R') => self.refresh(),

            // The play queue, ytmtui style: z opens the queue page, X queues
            // the video to play next, Z appends it at the end.
            KeyCode::Char('z') => self.open_queue(),
            KeyCode::Char('X') => self.queue_selected_next(),
            KeyCode::Char('Z') => self.queue_selected_append(),

            // Paging through long lists, yt-x style: N next page, P previous.
            KeyCode::Char('N') => self.page_feed(true),
            KeyCode::Char('P') => self.page_feed(false),
            KeyCode::Char('c') => self.open_selected_channel(),
            KeyCode::Char('y') => self.copy_selected_url(),
            KeyCode::Char('o') => self.open_selected_browser(),

            // Back only where there is somewhere to go back to; Esc cancels a
            // running fetch first, then steps back.
            KeyCode::Char('b') if !self.stack.is_empty() => self.back(),
            KeyCode::Esc => {
                if self.loading {
                    self.cancel_pending_fetch();
                } else if !self.stack.is_empty() || self.view == View::Browse {
                    self.back();
                }
            }
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        match overlay {
            Overlay::Search(mut input) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let query = input.value.trim().to_string();
                    if query.is_empty() {
                        self.overlay = Some(Overlay::Search(input));
                        return;
                    }
                    let outcome = if Self::looks_like_url(&query) {
                        self.start_url(&query)
                    } else {
                        self.start_search(&query)
                    };
                    if let Err(error) = outcome {
                        self.toast_error(error.to_string());
                        self.overlay = Some(Overlay::Search(input));
                    }
                }
                _ => {
                    input.handle_key(&key);
                    self.overlay = Some(Overlay::Search(input));
                }
            },
            Overlay::Quality { mut index } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Up | KeyCode::Char('k') => {
                    index = index.saturating_sub(1);
                    self.overlay = Some(Overlay::Quality { index });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    index = (index + 1).min(QUALITIES.len().saturating_sub(1));
                    self.overlay = Some(Overlay::Quality { index });
                }
                KeyCode::Enter => {
                    let height = QUALITIES
                        .get(index)
                        .map(|&(_, height)| height)
                        .filter(|&height| height > 0);
                    self.download_selected(false, height);
                }
                _ => {
                    self.overlay = Some(Overlay::Quality { index });
                }
            },
            Overlay::Help => match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter => {}
                _ => self.overlay = Some(Overlay::Help),
            },
        }
    }

    fn play_selected(&mut self, audio: bool) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        self.record_watch(&video);
        self.pending_effect = Some(Effect::Play {
            videos: vec![video],
            audio,
        });
    }

    /// `d` and the menu's Download row open the quality picker on the right;
    /// Enter on a row downloads with that height cap.
    fn open_quality(&mut self) {
        if self.download.is_some() {
            self.toast_error("A download is already running");
            return;
        }
        if self.selected_video().is_none() {
            self.toast_info("Nothing selected");
            return;
        }
        self.overlay = Some(Overlay::Quality { index: 0 });
    }

    fn download_selected(&mut self, audio: bool, height: Option<u32>) {
        if self.download.is_some() {
            self.toast_error("A download is already running");
            return;
        }
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        self.record_watch(&video);
        let job_id = self.begin_download(&video);
        self.pending_effect = Some(Effect::Download {
            job_id,
            video,
            audio,
            height,
        });
        let message = if audio {
            "Downloading audio…".to_string()
        } else {
            match height {
                Some(height) => format!("Downloading {height}p…"),
                None => "Downloading video…".to_string(),
            }
        };
        self.toast_info(message);
    }

    fn toggle_selected_saved(&mut self) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        let saved = self.store.toggle_saved(&video);
        let _ = self.persist();
        self.prune_saved_feed(&video.id);
        if saved {
            self.toast_success("Saved to your collection");
        } else {
            self.toast_info("Removed from saved videos");
        }
    }

    /// `r` removes the selected video from Saved (ytmtui's remove key) and
    /// says so when it is not saved in the first place.
    fn remove_selected_saved(&mut self) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        if !self.store.is_saved(&video.id) {
            self.toast_info("Not in saved videos");
            return;
        }
        self.store.toggle_saved(&video);
        let _ = self.persist();
        self.prune_saved_feed(&video.id);
        self.toast_info("Removed from saved videos");
    }

    /// Drop a video from the visible Saved feed when it just left the store.
    fn prune_saved_feed(&mut self, video_id: &str) {
        if self.feed.kind != FeedKind::Saved || self.store.is_saved(video_id) {
            return;
        }
        if let Some(position) = self.feed.items.iter().position(|item| item.id == video_id) {
            self.feed.items.remove(position);
            self.feed.subtitle = format!("{} VIDEOS", self.feed.items.len());
            if self.feed.items.is_empty() {
                self.feed.notice =
                    Some("Nothing saved yet. Press `a` on a result to keep it here.".to_string());
            }
            self.fix_selection_after_removal();
        }
    }

    /// Like state lives on the account when syncing is on: press `l` to like,
    /// `l` again to unlike. The local store mirrors the account state.
    fn toggle_like_selected(&mut self) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        let on = !self.store.is_liked(&video.id);
        self.store.set_like(&video, on);
        let _ = self.persist();
        self.dispatch_sync(
            &video,
            SyncAction::Like {
                video_id: video.id.clone(),
                on,
            },
            on,
            "Liked",
            "Removed like",
        );
    }

    /// `W` queues the selected video for Watch Later (`W` again removes it).
    fn toggle_watch_later_selected(&mut self) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        let on = !self.feed_is_watch_later_page() && !self.store.is_watch_later(&video.id);
        self.store.set_watch_later(&video, on);
        let _ = self.persist();
        self.dispatch_sync(
            &video,
            SyncAction::WatchLater {
                video_id: video.id.clone(),
                add: on,
            },
            on,
            "Added to Watch Later",
            "Removed from Watch Later",
        );
    }

    /// Fallback when account sync is unavailable: queue/like locally and say so.
    fn queue_locally(&mut self, on: bool, added: &str, removed: &str) {
        let reason = if self.config.account_sync {
            "set cookies_browser for account sync"
        } else {
            "account_sync is off in config.toml"
        };
        let text = if on {
            format!("{added} (local · {reason})")
        } else {
            format!("{removed} (local · {reason})")
        };
        self.toast_info(text);
    }

    fn dispatch_sync(
        &mut self,
        video: &Video,
        action: SyncAction,
        on: bool,
        added: &str,
        removed: &str,
    ) {
        if !self.sync_enabled() {
            self.queue_locally(on, added, removed);
            return;
        }
        self.toast_info("Syncing with your YouTube account…");
        self.pending_effect = Some(Effect::Sync {
            video: video.clone(),
            action,
        });
    }

    fn sync_enabled(&self) -> bool {
        self.config.account_sync && self.config.cookies_browser.is_some()
    }

    fn feed_is_watch_later_page(&self) -> bool {
        self.feed.source.contains("list=WL")
    }

    fn feed_is_liked_page(&self) -> bool {
        self.feed.source.contains("list=LL")
    }

    fn open_selected_channel(&mut self) {
        let Some(video) = self.selected_video().cloned() else {
            return;
        };
        let Some(channel_url) = video.channel_url else {
            self.toast_error("No channel URL attached to this video");
            return;
        };
        let mut url = channel_url.trim_end_matches('/').to_string();
        if !url.ends_with("/videos") {
            url.push_str("/videos");
        }
        self.home_origin = None;
        self.request_feed(
            url,
            format!("Channel · {}", video.channel),
            "CHANNEL VIDEOS".to_string(),
            FeedKind::Channel,
            false,
            true,
            false,
            self.view,
        );
    }

    fn copy_selected_url(&mut self) {
        let Some(video) = self.selected_video() else {
            return;
        };
        self.pending_effect = Some(Effect::Copy(video.url.clone()));
    }

    fn open_selected_browser(&mut self) {
        let Some(video) = self.selected_video() else {
            return;
        };
        self.pending_effect = Some(Effect::OpenUrl(video.url.clone()));
    }

    /// Move the selection, wrapping like ytmtui's next/previous: down from
    /// the last row lands on the first, up from the first on the last.
    fn move_selection(&mut self, delta: isize) {
        if self.feed.items.is_empty() {
            self.table_state.select(None);
            return;
        }
        let len = self.feed.items.len();
        let current = self.table_state.selected().unwrap_or(0);
        let steps = delta.unsigned_abs() % len;
        let next = if delta < 0 {
            // Up: wrap backwards so stepping past the top lands at the end.
            (current + len - steps) % len
        } else {
            // Down: wrap forwards so stepping past the end lands at the top.
            (current + steps) % len
        };
        self.table_state.select(Some(next));
    }

    fn select_first(&mut self) {
        if !self.feed.items.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    fn select_last(&mut self) {
        if !self.feed.items.is_empty() {
            self.table_state.select(Some(self.feed.items.len() - 1));
        }
    }

    fn reset_selection(&mut self) {
        if self.feed.items.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(0));
        }
    }

    fn fix_selection_after_removal(&mut self) {
        if self.feed.items.is_empty() {
            self.table_state.select(None);
            return;
        }
        let last = self.feed.items.len() - 1;
        let selected = self.table_state.selected().unwrap_or(0).min(last);
        self.table_state.select(Some(selected));
    }

    /// Re-query the current feed at a given page window without touching the
    /// back stack — the yt-x "Next/Previous" model: same list, shifted
    /// `--playlist-start/--playlist-end` window.
    fn refetch_page(&mut self, page: usize, bypass_cache: bool) {
        let url = self.feed.source.clone();
        let title = self.feed.title.clone();
        let subtitle = self.feed.subtitle.clone();
        let kind = self.feed.kind;
        let auth = self.feed.auth;

        self.fetch_id = self.fetch_id.wrapping_add(1);
        let id = self.fetch_id;
        self.loading = true;
        self.loading_label = if page > 1 {
            format!("Loading page {page}")
        } else {
            match kind {
                FeedKind::Search => "Searching YouTube".to_string(),
                FeedKind::Channel => "Loading channel".to_string(),
                FeedKind::Playlist => "Loading playlist".to_string(),
                _ => "Fetching with yt-dlp".to_string(),
            }
        };
        self.feed_page = page;
        self.feed_last_page = false;
        self.feed = Feed {
            title,
            source: url.clone(),
            kind,
            subtitle,
            items: Vec::new(),
            notice: None,
            auth,
            fetched_at: 0,
        };
        self.overlay = None;
        self.reset_selection();

        let mut spec = FetchSpec::new(id, url, self.feed.title.clone());
        spec.limit = self.config.results;
        spec.page = page;
        spec.cookies_browser = self.config.cookies_browser.clone();
        spec.requires_auth = auth;
        spec.cache_dir = Some(self.paths.feed_cache_dir());
        spec.bypass_cache = bypass_cache;
        self.pending_fetch = Some(spec);
    }

    /// Move one page through the current remote list: `N` forward, `P` back —
    /// the keyboard twin of yt-x's Next/Previous menu entries. Local lists
    /// (Saved, History, Queue) hold everything at once and never page.
    pub fn page_feed(&mut self, forward: bool) {
        if self.loading || self.feed.source.is_empty() {
            return;
        }
        if matches!(
            self.feed.kind,
            FeedKind::Saved | FeedKind::History | FeedKind::Queue
        ) {
            self.toast_info("This list holds all its items — no pages");
            return;
        }
        if self.feed.auth && self.config.cookies_browser.is_none() {
            return;
        }
        if forward && self.feed_last_page {
            self.toast_info("Last page");
            return;
        }
        let page = if forward {
            self.feed_page + 1
        } else if self.feed_page > 1 {
            self.feed_page - 1
        } else {
            return;
        };
        self.refetch_page(page, false);
    }

    fn refresh(&mut self) {
        match self.feed.kind {
            FeedKind::Saved => {
                self.show_saved(false);
                self.toast_info("Library refreshed");
            }
            FeedKind::History => {
                self.show_history(false);
                self.toast_info("History refreshed");
            }
            FeedKind::Queue => {
                self.show_queue(false);
                self.toast_info("Queue refreshed");
            }
            _ => {
                if self.feed.source.is_empty() {
                    return;
                }
                // Refresh keeps the page you are on.
                self.refetch_page(self.feed_page, true);
            }
        }
    }

    fn back(&mut self) {
        self.cancel_pending_fetch();
        if let Some(feed) = self.stack.pop() {
            self.feed = feed;
            // Keep the current view: popping from Home returns to Home with
            // the bar intact; popping from Browse stays in Browse.
            self.overlay = None;
            self.reset_selection();
        } else {
            self.view = View::Home;
            self.feed = Feed::empty();
            self.overlay = None;
            self.table_state.select(None);
        }
    }

    fn cancel_pending_fetch(&mut self) {
        self.fetch_id = self.fetch_id.wrapping_add(1);
        self.pending_fetch = None;
        self.loading = false;
    }

    pub fn handle_msg(&mut self, message: Msg) {
        match message {
            Msg::Feed { id, result } => {
                if id != self.fetch_id {
                    return;
                }
                self.loading = false;
                match result {
                    Ok(data) => {
                        self.feed.title = if matches!(self.feed.kind, FeedKind::Search) {
                            self.feed.title.clone()
                        } else {
                            data.title.clone()
                        };
                        self.feed.subtitle = format!(
                            "{}  ·  {} RESULTS{}",
                            self.feed.kind.label(),
                            data.items.len(),
                            if self.feed_page > 1 {
                                format!("  ·  PAGE {}", self.feed_page)
                            } else {
                                String::new()
                            }
                        );
                        // A short page means the list ends here: `N` stops
                        // offering another one.
                        self.feed_last_page =
                            self.feed_page > 1 && data.items.len() < self.config.results;
                        self.feed.notice = if data.items.is_empty() {
                            if self.feed_page > 1 {
                                Some(format!(
                                    "Page {} is empty — press `P` for the previous page.",
                                    self.feed_page
                                ))
                            } else {
                                Some(
                                    "No items returned. Try a different query or refresh with `r`."
                                        .to_string(),
                                )
                            }
                        } else {
                            None
                        };
                        self.feed.items = data.items;
                        self.feed.fetched_at = data.fetched_at;
                        self.reset_selection();
                        self.remember_last_feed();
                    }
                    Err(error) => {
                        self.feed.notice = Some(error.clone());
                        self.toast_error(error);
                    }
                }
            }
            Msg::DownloadProgress { id, percent } => self.update_download_percent(id, percent),
            Msg::DownloadDone { id, ok, message } => self.finish_download(id, ok, message),
            Msg::Thumbnail { id, result } => {
                let waiting = matches!(
                    &self.thumbnail,
                    ThumbnailState::Loading { id: current } if current == &id
                );
                if !waiting {
                    return;
                }
                match result {
                    Ok(image) => {
                        match self
                            .picker
                            .as_ref()
                            .map(|picker| picker.new_resize_protocol(image))
                        {
                            Some(protocol) => {
                                self.thumbnail = ThumbnailState::Ready { id, protocol }
                            }
                            None => self.thumbnail = ThumbnailState::Failed { id },
                        }
                    }
                    Err(_) => self.thumbnail = ThumbnailState::Failed { id },
                }
            }
            Msg::SyncDone {
                video_id,
                action,
                ok,
                message,
            } => {
                let finding = self
                    .feed
                    .items
                    .iter()
                    .find(|item| item.id == video_id)
                    .cloned();
                match action {
                    SyncAction::Like { on, .. } => {
                        self.finish_sync(&video_id, finding.as_ref(), true, on, ok, &message)
                    }
                    SyncAction::WatchLater { add, .. } => {
                        self.finish_sync(&video_id, finding.as_ref(), false, add, ok, &message)
                    }
                }
            }
            Msg::AccountName(name) => self.account_name = name,
        }
    }

    /// Notice wallpaper-tool palette changes while the TUI runs. Throttled to
    /// one `stat()` per second so a stable palette costs nothing. If a writer
    /// is truncating the newest file, it is skipped until the write lands and
    /// the current colours stay on screen.
    /// Watch the followed palette file for edits. Returns true when the
    /// colours actually changed, so the main loop can redraw exactly then
    /// instead of repainting every frame.
    pub fn poll_theme(&mut self) -> bool {
        const POLL_INTERVAL: Duration = Duration::from_secs(1);
        if self.theme_poll_at.elapsed() < POLL_INTERVAL {
            return false;
        }
        self.theme_poll_at = Instant::now();

        // No theming tool at all: fall back to the terminal's own colours.
        let Some(latest) = Theme::latest_file() else {
            if self.theme_src.is_some() {
                self.theme_src = None;
                self.theme_mtime = None;
                let changed = self.theme != Theme::default();
                self.theme = Theme::default();
                return changed;
            }
            return false;
        };

        // Steady state: the file we already follow is still the newest. If
        // its mtime has not moved there is nothing else to inspect, and if it
        // is mid-write we keep the last good theme instead of falling back to
        // an older wallpaper palette.
        if self.theme_src.as_ref() == Some(&latest) {
            let Ok(meta) = std::fs::metadata(&latest) else {
                return false;
            };
            let mtime = meta.modified().ok();
            if self.theme_mtime == mtime {
                return false;
            }
            if let Some(theme) = Theme::reload_from(&latest) {
                let changed = self.theme != theme;
                self.theme = theme;
                self.theme_mtime = mtime;
                return changed;
            }
            return false;
        }

        let Some(path) = Theme::source_file() else {
            // A palette exists but none parses yet: a writer is probably
            // mid-update. Keep the last good theme unless its file is gone.
            if self.theme_src.as_ref().is_some_and(|src| !src.exists()) {
                self.theme_src = None;
                self.theme_mtime = None;
                let changed = self.theme != Theme::default();
                self.theme = Theme::default();
                return changed;
            }
            return false;
        };

        // If we can't stat it, leave the watched state alone and try again
        // next second rather than latching on a missing mtime.
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        let mtime = meta.modified().ok();
        if self.theme_src.as_ref() == Some(&path) && self.theme_mtime == mtime {
            return false;
        }
        // Only latch the path/mtime once the palette parsed. A failed read
        // means the writer is still going; the next poll retries.
        let mut changed = false;
        if let Some(theme) = Theme::reload_from(&path) {
            changed = self.theme != theme;
            self.theme = theme;
            self.theme_src = Some(path);
            self.theme_mtime = mtime;
        }
        changed
    }

    /// Applies one completed sync action to the local mirror: keep the state on
    /// success, roll it back on failure, and refresh account pages that list
    /// the thing we just changed on the server.
    fn finish_sync(
        &mut self,
        video_id: &str,
        video: Option<&Video>,
        like: bool,
        on: bool,
        ok: bool,
        message: &str,
    ) {
        match ok {
            true => {
                if let Some(video) = video {
                    if like {
                        self.store.set_like(video, on);
                    } else {
                        self.store.set_watch_later(video, on);
                    }
                    let _ = self.persist();
                }
                let done = match (like, on) {
                    (true, true) => "Liked on YouTube",
                    (true, false) => "Like removed on YouTube",
                    (false, true) => "Added to Watch Later on YouTube",
                    (false, false) => "Removed from Watch Later on YouTube",
                };
                self.toast_success(done);
                self.refresh_after_sync(video_id);
            }
            false => {
                if let Some(video) = video {
                    if like {
                        self.store.set_like(video, !on);
                    } else {
                        self.store.set_watch_later(video, !on);
                    }
                    let _ = self.persist();
                    if self.feed_is_liked_page() || self.feed_is_watch_later_page() {
                        self.refresh_after_sync(video_id);
                    }
                }
                self.toast_error(message.to_string());
            }
        }
    }

    /// Re-fetch the account page whose membership just changed on the server.
    fn refresh_after_sync(&mut self, video_id: &str) {
        let page_changed = (self.feed_is_watch_later_page() || self.feed_is_liked_page())
            && self.feed.items.iter().any(|item| item.id == video_id);
        if page_changed {
            self.refresh();
        }
    }

    pub fn begin_download(&mut self, video: &Video) -> u64 {
        self.download_id = self.download_id.wrapping_add(1);
        let job_id = self.download_id;
        self.download = Some(DownloadView {
            job_id,
            title: video.title.clone(),
            percent: 0,
        });
        job_id
    }

    pub fn record_watch(&mut self, video: &Video) {
        self.store.record_watch(video);
        let _ = self.persist();
    }

    pub fn persist(&self) -> Result<()> {
        self.store.save(&self.paths.state_file())
    }

    pub fn toast_info(&mut self, message: impl Into<String>) {
        self.set_toast(message, ToastKind::Info);
    }

    pub fn toast_success(&mut self, message: impl Into<String>) {
        self.set_toast(message, ToastKind::Success);
    }

    pub fn toast_error(&mut self, message: impl Into<String>) {
        self.set_toast(message, ToastKind::Error);
    }

    fn set_toast(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            text: message.into(),
            kind,
            created: Instant::now(),
        });
    }

    pub fn open_search_overlay(&mut self) {
        self.overlay = Some(Overlay::Search(TextInput::default()));
    }

    pub fn copy_url(&mut self, url: &str) {
        match copy_to_clipboard(url) {
            Ok(()) => self.toast_success("URL copied to clipboard"),
            Err(error) => self.toast_error(error.to_string()),
        }
    }

    pub fn update_download_percent(&mut self, job_id: u64, percent: u8) {
        if let Some(download) = self.download.as_mut() {
            if download.job_id == job_id {
                download.percent = percent;
            }
        }
    }

    pub fn finish_download(&mut self, job_id: u64, ok: bool, message: String) {
        if self
            .download
            .as_ref()
            .map(|download| download.job_id == job_id)
            .unwrap_or(false)
        {
            self.download = None;
        }
        if ok {
            self.toast_success(message);
        } else {
            self.toast_error(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn text_input_handles_wide_characters() {
        let mut input = TextInput::default();
        for character in "aé日".chars() {
            input.handle_key(&key(KeyCode::Char(character)));
        }
        assert_eq!(input.value, "aé日");
        assert_eq!(input.cursor, 3);
        input.handle_key(&key(KeyCode::Backspace));
        assert_eq!(input.value, "aé");
        input.handle_key(&key(KeyCode::Home));
        input.handle_key(&key(KeyCode::Delete));
        assert_eq!(input.value, "é");
    }

    #[test]
    fn text_input_deletes_word() {
        let mut input = TextInput {
            value: "rust programming".to_string(),
            cursor: 16,
        };
        input.handle_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.value, "rust ");
    }

    fn app() -> App {
        let root = std::env::temp_dir().join(format!("utui-app-test-{}", std::process::id()));
        App::new(
            crate::config::Paths {
                config_dir: root.join("config"),
                data_dir: root.join("data"),
                cache_dir: root.join("cache"),
            },
            crate::config::Config::default(),
            crate::store::Store::default(),
        )
    }

    fn watch_later_feed() -> Feed {
        Feed {
            title: "Watch later videos".into(),
            source: "https://www.youtube.com/playlist?list=WL".into(),
            kind: FeedKind::Playlist,
            subtitle: "WATCH LATER  ·  50 RESULTS".into(),
            items: Vec::new(),
            notice: None,
            auth: true,
            fetched_at: 0,
        }
    }

    fn videos(count: usize) -> Vec<Video> {
        (0..count)
            .map(|i| Video {
                id: format!("v{i}"),
                title: format!("Video {i}"),
                ..Video::default()
            })
            .collect()
    }

    #[test]
    fn paging_walks_windows_like_yt_x() {
        let mut app = app();
        app.config.cookies_browser = Some("firefox".into());
        app.feed = watch_later_feed();

        // N queues the next window without pushing the back stack.
        app.page_feed(true);
        assert_eq!(app.feed_page, 2);
        let spec = app.pending_fetch.take().expect("page-2 fetch queued");
        assert_eq!(spec.page, 2);
        assert_eq!(spec.limit, 50);
        assert!(!spec.bypass_cache);
        assert!(app.stack.is_empty(), "paging must not push history");

        // A full page lands: the marker shows and the next one is offered.
        app.fetch_id = spec.id;
        app.handle_msg(Msg::Feed {
            id: spec.id,
            result: Ok(FeedData {
                title: "Watch later videos".into(),
                source: "https://www.youtube.com/playlist?list=WL".into(),
                items: videos(50),
                fetched_at: 0,
            }),
        });
        assert_eq!(app.feed_page, 2);
        assert!(!app.feed_last_page, "a full page is not the last");
        assert!(
            app.feed.subtitle.contains("PAGE 2"),
            "{}",
            app.feed.subtitle
        );
        assert_eq!(app.feed.items.len(), 50);

        // A short page means the end: N queues nothing more.
        app.page_feed(true);
        let spec = app.pending_fetch.take().expect("page-3 fetch queued");
        app.fetch_id = spec.id;
        app.handle_msg(Msg::Feed {
            id: spec.id,
            result: Ok(FeedData {
                title: "Watch later videos".into(),
                source: "https://www.youtube.com/playlist?list=WL".into(),
                items: videos(12),
                fetched_at: 0,
            }),
        });
        assert!(app.feed_last_page);
        app.page_feed(true);
        assert!(app.pending_fetch.is_none(), "no fetch past the last page");
        assert_eq!(app.feed_page, 3);

        // P walks back one window at a time.
        app.page_feed(false);
        assert_eq!(app.feed_page, 2);
        let spec = app.pending_fetch.take().expect("back-page fetch queued");
        assert_eq!(spec.page, 2);
    }

    #[test]
    fn local_lists_and_first_page_never_page() {
        let mut app = app();
        app.feed = Feed {
            title: "Saved videos".into(),
            source: "local:saved".into(),
            kind: FeedKind::Saved,
            subtitle: "SAVED  ·  3 RESULTS".into(),
            items: videos(3),
            notice: None,
            auth: false,
            fetched_at: 0,
        };
        app.page_feed(true);
        assert_eq!(app.feed_page, 1, "Saved holds everything at once");
        assert!(app.pending_fetch.is_none());

        // P on the first page is a no-op too.
        app.feed = watch_later_feed();
        app.page_feed(false);
        assert_eq!(app.feed_page, 1);
        assert!(app.pending_fetch.is_none());

        // Refresh keeps the page it was asked on.
        app.feed_page = 3;
        app.refresh();
        let spec = app.pending_fetch.take().expect("refresh queues a fetch");
        assert_eq!(spec.page, 3);
        assert!(spec.bypass_cache);
    }

    fn search_feed() -> Feed {
        Feed {
            title: "Search · rust".into(),
            source: "https://example.com".into(),
            kind: FeedKind::Search,
            subtitle: "SEARCH  ·  3 RESULTS".into(),
            items: videos(3),
            notice: None,
            auth: false,
            fetched_at: 0,
        }
    }

    #[test]
    fn x_queues_next_and_z_appends() {
        let mut app = app();
        app.feed = search_feed();
        app.table_state.select(Some(0));
        app.handle_key(key(KeyCode::Char('X')));
        assert_eq!(
            app.store
                .queue
                .iter()
                .map(|video| video.id.as_str())
                .collect::<Vec<_>>(),
            vec!["v0"],
            "X queues the video to play next"
        );

        app.table_state.select(Some(2));
        app.handle_key(key(KeyCode::Char('Z')));
        assert_eq!(
            app.store
                .queue
                .iter()
                .map(|video| video.id.as_str())
                .collect::<Vec<_>>(),
            vec!["v0", "v2"],
            "Z appends at the end"
        );

        // The queue survives a refresh of the feed and can be re-queued.
        app.handle_key(key(KeyCode::Char('X')));
        assert_eq!(app.store.queue.len(), 3, "duplicates are allowed");
        assert_eq!(app.store.queue[0].id, "v2");
    }

    #[test]
    fn z_opens_the_queue_page_and_b_comes_back() {
        let mut app = app();
        app.feed = search_feed();
        app.table_state.select(Some(1));

        app.handle_key(key(KeyCode::Char('z')));
        assert_eq!(app.feed.kind, FeedKind::Queue);
        assert_eq!(app.feed.source, "utui://queue");
        assert!(app.feed.items.is_empty(), "nothing queued yet");
        assert_eq!(
            app.stack.len(),
            1,
            "the feed you came from stays on the back stack"
        );

        // b returns to the feed you opened the queue from.
        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(app.feed.kind, FeedKind::Search);
        assert!(app.stack.is_empty());
    }

    #[test]
    fn queue_page_edits_reorder_remove_and_play_the_tail() {
        let mut app = app();
        app.store.queue = videos(3);
        app.open_queue();
        app.table_state.select(Some(0));

        // J/K move the selected row down and up, persisting the order.
        app.handle_key(key(KeyCode::Char('J')));
        assert_eq!(
            app.store
                .queue
                .iter()
                .map(|video| video.id.as_str())
                .collect::<Vec<_>>(),
            vec!["v1", "v0", "v2"]
        );
        assert_eq!(
            app.table_state.selected(),
            Some(1),
            "the row travels with J"
        );
        app.handle_key(key(KeyCode::Char('K')));
        assert_eq!(app.store.queue[0].id, "v0");

        // d removes the selected row.
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(
            app.store
                .queue
                .iter()
                .map(|video| video.id.as_str())
                .collect::<Vec<_>>(),
            vec!["v1", "v2"],
            "d drops the row from the queue"
        );
        assert_eq!(app.table_state.selected(), Some(0));

        // Enter plays the queue from the highlighted row to the end.
        app.handle_key(key(KeyCode::Char('J')));
        assert_eq!(app.table_state.selected(), Some(1));
        // J swapped v2 to the front, so row 1 now holds v1 — Enter plays
        // from there on.
        app.handle_key(key(KeyCode::Enter));
        match app.take_effect() {
            Some(Effect::Play { videos, audio }) => {
                assert!(!audio);
                assert_eq!(
                    videos
                        .iter()
                        .map(|video| video.id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["v1"],
                    "Enter plays from here on, not just one video"
                );
            }
            other => panic!("expected a play effect, got {other:?}"),
        }

        // A on the queue page plays the tail as audio.
        app.table_state.select(Some(0));
        app.handle_key(key(KeyCode::Char('A')));
        match app.take_effect() {
            Some(Effect::Play { videos, audio }) => {
                assert!(audio);
                assert_eq!(videos.len(), 2, "v1 and v2 play audio-only");
            }
            other => panic!("expected a play effect, got {other:?}"),
        }
    }

    #[test]
    fn queue_page_reset_the_window_and_never_pages() {
        let mut app = app();
        app.feed = watch_later_feed();
        app.feed_page = 3;
        app.feed_last_page = true;
        app.store.queue = videos(2);

        // Opening the queue over a paged feed starts back at page 1.
        app.open_queue();
        assert_eq!(app.feed_page, 1, "the queue never inherits a page window");
        assert!(!app.feed_last_page);

        // N/P stay quiet on the queue page.
        app.page_feed(true);
        assert_eq!(app.feed_page, 1);
        assert!(app.pending_fetch.is_none());

        // The queue is not remembered as the launch feed.
        app.handle_key(key(KeyCode::Char('b')));
        app.store.last_feed = None;
        app.open_queue();
        assert!(
            app.store.last_feed.is_none(),
            "the queue is not a launch feed"
        );
    }

    #[test]
    fn moving_selection_wraps_around_both_ends() {
        let mut app = app();
        app.feed = search_feed(); // three videos: v0 v1 v2
        app.table_state.select(Some(0));

        // j at the bottom lands on the top (loop scroll).
        app.table_state.select(Some(2));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.table_state.selected(), Some(0), "j past the end wraps");

        // k at the top lands on the bottom.
        app.table_state.select(Some(0));
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.table_state.selected(), Some(2), "k past the top wraps");

        // Inside the list it steps normally, no wrapping.
        app.table_state.select(Some(1));
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.table_state.selected(), Some(2), "j steps down in range");
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.table_state.selected(), Some(1), "k steps up in range");

        // A ten-row page step wraps too: from row 2, PageUp lands on row 1
        // ((2 - 10) mod 3 == 1).
        app.table_state.select(Some(2));
        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.table_state.selected(), Some(1), "PageUp wraps the page");

        // An empty list never crashes on a move.
        app.feed.items.clear();
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.table_state.selected(), None);
    }
}
