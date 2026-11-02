# Utui

**Utui** is a blazing-fast terminal YouTube browser written in Rust.

The classic terminal-YouTube workflow as a single native binary: no shell script, no `fzf`, no `jq`, no pipeline juggling. `yt-dlp` does extraction, `mpv` does playback, and a ratatui interface keeps the whole flow fast and keyboard-first.

## Features

- **Native TUI** — ratatui + crossterm with a minimal, terminal-default palette and a single accent color. No external launcher.
- **Fast search** — one `yt-dlp --flat-playlist --dump-single-json` call, parsed in-process and cached for 10 minutes.
- **Thumbnails** — the selected video's thumbnail renders in the preview pane beside the list using kitty, sixel, or iTerm2 graphics when the terminal supports them, with unicode half-blocks as fallback. Downloads are cached in `~/.cache/utui/thumbs`.
- **Search filters** — YouTube's colon syntax: `:today`, `:week`, `:4k`, `:hd`, `:live`, `:short`, `:newest`, `:views`, and more.
- **Search recall** — `!1`, `!2`, … replay previous searches.
- **Channels and playlists** — open a channel from any result or paste any yt-dlp URL.
- **Playback** — video with `mpv`, or audio-only mode. Utui hands the URL straight to the player.
- **Downloads** — individual video or MP3 audio, organized under `download_dir/video` and `download_dir/audio`, with live progress in the status bar.
- **Library** — locally saved videos and watch history, stored as JSON.
- **Play queue** — `X` queues a video to play next, `Z` queues it at the end, and `z` opens the queue as its own page where `J`/`K` reorder, `d` removes, and `Enter` hands the whole tail to `mpv` as one playlist. The queue is stored with the rest of your state and survives restarts.
- **Cookie-backed feeds** — the YouTube home feed (My Feeds), Subscriptions, Watch Later, and Liked videos via `yt-dlp --cookies-from-browser`.
- **Feeds on launch** — Utui reopens the feed you were viewing when you last quit, so content is on screen immediately.
- **Wallpaper theming** — [pywal](https://github.com/dylanaraps/pywal), [wallust](https://github.com/Fuffy/wallust), and [matugen](https://github.com/0xAdarsh/matugen) are all detected: their cache/config directories and JSON template outputs are scanned, the newest valid palette wins, and it is **reloaded live** — change your wallpaper and the running TUI repaints within a second, no restart. Text, accent, warning, error, and dim map to palette colors, while the background stays your terminal's own (transparency included). Without any theming tool, Utui uses your terminal's own colors with a cyan accent.
- **Signed-in account in the header** — when `cookies_browser` is configured, Utui reads your YouTube account name in the background and shows it on the header line above the search input: `UTUI ⟢ <your name>`.
- **Scriptable** — `--json` gives clean machine-readable output for scripts and pipelines.
- **Small and fast** — release builds use LTO and a single codegen unit. The only HTTP traffic is thumbnail downloads from YouTube's image CDN (over one shared HTTP connection pool). The TUI redraws only when something changes — an idle screen costs nothing to keep open, and the loading spinner keeps animating at 10 fps only while it is visible — and both on-disk caches (feeds and thumbnails) are capped, so `~/.cache/utui` never grows without bound.

## Requirements

| Tool | Why |
| --- | --- |
| `yt-dlp` | Extract search results, playlists, channels, and media |
| `mpv` | Play video and audio |
| `ffmpeg` | Merge video/audio and create MP3 downloads |
| `wl-clipboard`, `xclip`, or `xsel` | Optional, for `y` copy URL |

Rust 1.86 or newer is required to build.

## Install

```bash
git clone https://github.com/castillo/utui
cd utui
cargo build --release
install -Dm755 target/release/utui ~/.local/bin/utui
```

Make sure `~/.local/bin` is on your `PATH`.

### Arch Linux (AUR)

Utui is on the [AUR](https://aur.archlinux.org/) as [`utui`](https://aur.archlinux.org/packages/utui):

```bash
paru -S utui            # with paru
yay -S utui             # with yay
```

Or clone the AUR repo and build it yourself:

```bash
git clone https://aur.archlinux.org/utui.git
cd utui
makepkg -si
```

## Usage

```bash
utui                     # start on the home screen
utui setup               # configure YouTube sign-in (interactive)
utui --search "rust"     # open search results immediately
utui --url "https://www.youtube.com/playlist?list=..."  # open a URL
```

### Headless / scripting

```bash
utui --search "lofi :today" --json --limit 10
utui --url "https://www.youtube.com/watch?v=..." --json
```

The JSON output includes `title`, `source`, `count`, and an `items` array with the same fields Utui uses internally.

### Useful flags

| Flag | Description |
| --- | --- |
| `-s`, `--search <QUERY>` | Start in search results |
| `-u`, `--url <URL>` | Open a video, playlist, channel, or any yt-dlp URL |
| `--json` | Print results as JSON, no TUI |
| `-n`, `--limit <N>` | Number of entries to request |
| `--cookies-from-browser <BROWSER>` | Override the configured browser for cookies |
| `--no-cache` | Ignore the feed cache for this run |
| `--clear-cache` | Delete cached feeds and exit (or continue if combined with a query) |

## Playback and downloads

- `Enter` or `Space` (or **Play video**) opens `mpv` with the best video + audio stream.
- `A` or **Play audio only** opens `mpv` with audio-only settings.
- `d` opens the quality picker on the right side (best / 2160p / 1440p / 1080p / 720p / 480p / 360p / 250p / 160p); `Enter` downloads that quality to `download_dir/video/`.
- `D` downloads MP3 audio to `download_dir/audio/`.
- The player and its arguments are fully configurable in `config.toml`.

While a download is running, progress appears in the status bar. Downloads never block the interface.

## Cookie-backed feeds

My Feeds, Subscriptions, Watch Later, and Liked videos need YouTube cookies. The setup wizard walks through all of it:

```bash
utui setup
```

It automatically scans every browser yt-dlp can read cookies from for a signed-in YouTube session, then lets you choose: each detected browser is listed with what the scan found (`✓ YouTube session found`, `✗ no YouTube session`, `? couldn't read cookies`, or `· no profile yet`), and the preselected default is the first browser that's actually signed in (falling back to your configured one, then the first browser found). If you're signed out everywhere, it offers to open the YouTube sign-in page and re-checks once you're done. It verifies the final choice with a live Watch Later fetch and writes the result to `~/.config/utui/config.toml`:

```toml
cookies_browser = "firefox"
account_sync = true
```

Supported values are whatever `yt-dlp --cookies-from-browser` accepts: `firefox`, `chrome`, `chromium`, `brave`, `edge`, `opera`, `safari`, `vivaldi`, or `whale`, optionally with a profile (`firefox::/path/to/profile`).

Useful variants:

| Command | What it does |
| --- | --- |
| `utui setup` | Interactive setup (scans browsers for sessions, lets you choose, verifies, saves) |
| `utui setup --status` | Show the configured browser, account-sync state, and session check without changing anything |
| `utui setup --browser chrome` | Configure non-interactively (exits non-zero if verification fails, after saving) |
| `utui setup --browser firefox --no-probe` | Configure without the live network check |
| `utui setup --browser chrome --account-sync false` | Configure and turn account sync off |

You can also set the value by hand in `config.toml` — run `utui setup --status` afterwards to verify it. Restart Utui for changes to apply.

Cookies are used only by `yt-dlp` and its cookie machinery (also by the account-sync helper, which is a standalone `yt-dlp`-based process). Utui never reads or stores browser cookies itself.

## Keybindings

Home and Browse share one key map: navigation and playback follow the usual player conventions — `j`/`k` and the arrows navigate, `Enter`/`Space` play, `/` searches, `Esc` steps back.

### Global

| Key | Action |
| --- | --- |
| `/` | Focus the search prompt (always visible; accepts queries, colon filters, and plain URLs) |
| `m` | Action menu for the selected video |
| `?` | Keybindings help |
| `q` | Quit |
| `Ctrl+C` | Quit |

### Lists (Home and Browse)

| Key | Action |
| --- | --- |
| `j` `n` `↓` `→` `Tab` | Move down (wraps to the first row past the end) |
| `k` `h` `p` `↑` `←` | Move up (wraps to the last row past the top) |
| `g` / `G` (or `Home` / `End`) | First / last result |
| `PageUp` / `PageDown` | Page through results |
| `Enter` / `Space` | Play the selected video |
| `m` | Action menu (play, download, save, copy, …) |
| `d` | Open the quality picker (best / 2160p … 160p); `Enter` downloads it |
| `D` | Download audio (MP3) |
| `s` (or `a`) | Save or unsave the highlighted video |
| `r` | Remove the selected video from Saved |
| `l` (or `L`) | Like or unlike the highlighted video (YouTube account when syncing is on) |
| `w` (or `W`) | Add the highlighted video to / remove it from Watch Later (YouTube account when syncing is on) |
| `A` | Play audio only |
| `c` | Open the selected channel |
| `y` | Copy the video URL |
| `o` | Open in browser |
| `R` | Refresh the current feed (bypasses cache) |
| `N` / `P` | Next / previous page of the list — yt-x style windowed paging on My Feeds, Watch Later, Liked, channels, playlists, and search (pages of `results` from the config; a `page n · N next · P previous` banner names the window). Numbering is continuous across pages: page 1 is 1-50, page 2 is 51-100, and so on |
| `z` | Open the play queue (see below) |
| `X` | Queue the highlighted video to play next (at the front of the queue) |
| `Z` | Queue the highlighted video at the end of the queue |
| `b` / `Esc` | Back (with a stacked feed; `Esc` also cancels a running fetch) |

### Play queue (ytmtui's `z` page)

The play queue is a "watch these next" list you build while browsing. `X` queues the highlighted video to play next (front of the queue), `Z` queues it at the end — the same bindings ytmtui uses. `z` opens the queue as its own page; `b`/`Esc` steps back to wherever you came from.

On the queue page:

| Key | Action |
| --- | --- |
| `J` / `K` | Move the selected row down / up |
| `d` | Remove the selected row |
| `X` / `Z` | Move the selected row to the front / the end |
| `Enter` / `Space` | Play the queue from the highlighted video to the end — one `mpv` session, URLs back to back as a single playlist |
| `A` | Same, audio only |
| `b` / `Esc` | Back to the feed you opened it from |

The queue is saved in `state.json` and survives restarts; it is capped at 250 entries and never deduplicated (queueing the same video twice is intentional). When you press `Enter`, `mpv` receives every URL after the highlighted one in a single invocation and plays them in order; only the first video of the batch lands in History. A `queue · J/K move · d remove · ↵ play from here` banner names the keys on the queue page.

### Home

The interface is compact by design: the search prompt with its decorative rule up top, one line of feed shortcuts, the list with its preview pane, and a status line — nothing else. The rule wears the same pywal color (color6) ytmtui paints its own, and the status line keeps the keybind guide pinned to the right with the message on the left.

The search prompt is always visible in its own top row (placeholder: `/ search youtube · filters`). Press `/` to focus it — the row shows your query with a cursor and the status line switches to the live hint (`enter opens this url` once a URL is detected, `filter :…` once you type a colon filter).

My Feeds, Watch Later, Liked videos, Saved videos, and History sit on the shortcut line right under the search rule (`1. My Feeds  ·  2. Watch Later  ·  3. Liked videos  ·  4. Saved videos  ·  5. History`); the entry for the feed you have open is highlighted in the accent color. My Feeds is the YouTube home page itself and follows the same cookie rule as Watch Later and Liked videos — YouTube only serves a personalized home feed to signed-in visitors. Subscriptions stays reachable via the `f` shortcut (and via last-feed restore).

When Utui opens it lands in the last-viewed feed (see *Feeds on launch* below), so content is on screen immediately, with the selected video's thumbnail and details in the preview pane.

The feeds jump like ytmtui's `1`–`5` section keys:

| Key | Action |
| --- | --- |
| `1` / `2` / `3` / `4` / `5` | My Feeds / Watch Later / Liked / Saved / History |
| `F` / `S` / `H` / `f` | Extra letter shortcuts for My Feeds / Saved / History / Subscriptions |
| `Enter` with an empty list | Open the highlighted feed |

## Search filters

Place one filter at the start or end of a query:

```
synthwave :today
:4k drum and bass
rust programming :newest
```

Available filters:

```
:hour  :today  :week  :month  :year
:video  :movie  :live  :short  :long
:4k  :hd  :subtitles  :360  :vr  :3d  :hdr  :local
:newest  :views  :rating
```

## Configuration

The config file is created on first run at `~/.config/utui/config.toml`:

```toml
player = "mpv"
video_args = [
    "--no-terminal",
    "--force-window=yes",
    "--ytdl-format=bestvideo*+bestaudio/best",
]
audio_args = [
    "--no-terminal",
    "--no-video",
    "--ytdl-format=bestaudio/best",
]
results = 50
download_dir = "/home/you/Videos/utui"

# Optional, for My Feeds / Subscriptions / Watch Later / Liked videos:
# cookies_browser = "firefox"

# Write likes and Watch Later to your YouTube account (needs cookies_browser).
# account_sync = true
```

State lives in `~/.local/share/utui/state.json`. Feed cache lives in `~/.cache/utui/feeds` and thumbnail cache in `~/.cache/utui/thumbs`. `state.json` also remembers the last feed so the next launch reopens it (`1`–`5` bar pages by shortcut; searches and channels by source).

## Theme (pywal)

Utui reads `~/.cache/wal/colors.json` at startup:

| Interface role | pywal color |
| --- | --- |
| Text | `color7`, then `color15`, then `special.foreground` |
| Accent (keys, spinner, prompts) | `color4`, then `color6` |
| Second accent (decorative rule) | `color6`, then `color14` |
| Success / Warning / Error | `color2` / `color3` / `color1` |
| Dim text | `color8` |

No background is ever painted — the frame leaves every cell's background untouched, so your terminal's own surface (transparency, blur, wallpaper) shows through exactly as it does in a plain shell.

No file, no palette → your terminal's own defaults. `WAL_COLORS_FILE` overrides the path for testing.

## Account sync (real likes and Watch Later)

With `cookies_browser` set and `account_sync = true` — both written by `utui setup` — the `l` and `w` keys (uppercase `L` / `W` also work) act on your actual YouTube account, not just a local list:

- `l` likes the video; `l` again unlikes it.
- `w` adds the video to Watch Later; `w` again removes it.
- `1` opens the Watch Later queue from your account. On that page `w` removes the item.

How it works: Utui ships a small embedded helper that reuses `yt-dlp`'s cookie extraction (every browser and keyring yt-dlp supports just works), signs requests to YouTube's web API with the same `SAPISIDHASH` scheme the web client uses, and calls the internal like/Watch Later endpoints. The helper is materialized to `~/.cache/utui/sync.py`, so you can inspect exactly what gets sent, and each action prints one JSON result. The local store mirrors the account state so list markers stay accurate; if the account write fails, the mirror rolls back.

Notes:

- YouTube does not let Utui read your current like state cheaply, so the first `l` on a video you already liked elsewhere will unlike it. Open the Liked page to see the truth.
- Without `cookies_browser`, or with `account_sync = false`, `l` and `w` toggle local mirrors only (the `S` Saved-list equivalent lives locally) and a toast says so.
- Feeds for Watch Later and Liked videos remain read-through yt-dlp. After a successful sync on those pages, Utui refreshes the list automatically.

## How it compares to the shell-script workflow

Utui is an independent Rust implementation of the classic terminal-YouTube workflow. The core interaction model — search, feeds, channels, playback, downloads, and local history — is deliberately similar.

What is different:

- **One binary instead of a POSIX shell script.** No `fzf`, `jq`, or `curl` dependency at runtime.
- **In-process TUI.** Menus, previews, and actions are drawn by ratatui rather than delegated to a launcher.
- **Typed data model.** yt-dlp JSON is parsed once into a `Video` struct and reused for lists, details, cache, and storage.
- **Ten-minute feed cache.** Repeating a search or opening a channel again is instant until the cache expires.
- **Headless JSON mode.** Useful for scripts and for testing behavior without a terminal.

Not implemented yet: rofi launcher mode, auto-mix playlists, custom user extensions, and multi-language UI strings.

## Development

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```

The code is split into:

- `src/main.rs` — CLI, terminal lifecycle, playback, downloads
- `src/app.rs` — application state and key handling
- `src/ui.rs` — ratatui rendering
- `src/yt.rs` — yt-dlp integration, parsing, and cache
- `src/store.rs` — saved videos, history, and search history
- `src/config.rs` — paths and config
- `src/util.rs` — formatting and small helpers

## License

MIT. Utui is not affiliated with YouTube, yt-dlp, mpv, or any other project.
