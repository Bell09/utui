# Utui

**Utui** is a blazing-fast terminal YouTube browser written in Rust.

It reimplements the `yt-x` workflow as a single native binary: no shell script, no `fzf`, no `jq`, no pipeline juggling. `yt-dlp` does extraction, `mpv` does playback, and a ratatui interface keeps the whole flow fast and keyboard-first.

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
- **Cookie-backed feeds** — Subscriptions, Watch Later, and Liked videos via `yt-dlp --cookies-from-browser`.
- **Feeds on launch** — Utui reopens the feed you were viewing when you last quit, so content is on screen immediately.
- **pywal colors** — when `~/.cache/wal/colors.json` exists (written by [pywal](https://github.com/dylanaraps/pywal)), the wallpaper's 16-color palette drives the interface: the background is painted, text uses the palette foreground, and accent/success/warning/error/dim all map to palette colors. Without pywal, Utui uses your terminal's own colors with a cyan accent.
- **Scriptable** — `--json` gives clean machine-readable output for scripts and pipelines.
- **Small and fast** — release builds use LTO and a single codegen unit. The only HTTP traffic is thumbnail downloads from YouTube's image CDN.

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

## Usage

```bash
utui                     # start on the home screen
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
- `d` downloads video to `download_dir/video/`.
- `D` downloads MP3 audio to `download_dir/audio/`.
- The player and its arguments are fully configurable in `config.toml`.

While a download is running, progress appears in the status bar. Downloads never block the interface.

## Cookie-backed feeds

Subscriptions, Watch Later, and Liked videos need YouTube cookies.

1. Set your browser in `~/.config/utui/config.toml`:

   ```toml
   cookies_browser = "firefox"
   ```

   Supported values are whatever `yt-dlp --cookies-from-browser` accepts, for example `firefox`, `chrome`, `chromium`, `brave`, `edge`, `opera`, `safari`, or `vivaldi`.

2. Make sure that browser is signed in to YouTube.
3. Restart Utui (or restart your terminal session so the palette reloads).

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
| `j` `n` `↓` `→` `Tab` | Move down |
| `k` `h` `p` `↑` `←` | Move up |
| `g` / `G` (or `Home` / `End`) | First / last result |
| `PageUp` / `PageDown` | Page through results |
| `Enter` / `Space` | Play the selected video |
| `m` | Action menu (play, download, save, copy, …) |
| `d` / `D` | Download video / audio |
| `a` / `s` | Save or unsave the selected video |
| `r` | Remove the selected video from Saved |
| `L` | Like or unlike the selected video (YouTube account when syncing is on) |
| `W` | Add to / remove from Watch Later (YouTube account when syncing is on) |
| `A` | Play audio only |
| `c` | Open the selected channel |
| `y` | Copy the video URL |
| `o` | Open in browser |
| `R` | Refresh the current feed (bypasses cache) |
| `b` / `Esc` | Back (with a stacked feed; `Esc` also cancels a running fetch) |

### Home

The interface is compact by design: the search prompt up top, one line of feed shortcuts, the list with its preview pane, and a status line — nothing else.

The search prompt is always visible in its own top row (placeholder: `/ search youtube · paste a url · filters`). Press `/` to focus it — the row shows your query with a cursor and the status line switches to the live hint (`enter opens this url` for pasted links, `filter :…` once you type a colon filter).

Watch Later, Liked videos, Saved videos, and History sit on the shortcut line right under the search prompt (` w Watch Later  ·  l Liked videos  ·  S Saved videos  ·  H History`); the entry for the feed you have open is highlighted in the accent color. Subscriptions stays reachable via the hidden `f` shortcut (and via last-feed restore).

When Utui opens it lands in the last-viewed feed (see *Feeds on launch* below), so content is on screen immediately, with the selected video's thumbnail and details in the preview pane.

The feeds jump like ytmtui's `1`–`5` section keys:

| Key | Action |
| --- | --- |
| `1` / `2` / `3` / `4` / `5` | Watch Later / Liked / Saved / History / Subscriptions |
| `w` / `l` / `S` / `H` / `f` | The same feeds by letter |
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

# Optional, for Subscriptions / Watch Later / Liked videos:
# cookies_browser = "firefox"

# Write likes and Watch Later to your YouTube account (needs cookies_browser).
# account_sync = true
```

State lives in `~/.local/share/utui/state.json`. Feed cache lives in `~/.cache/utui/feeds` and thumbnail cache in `~/.cache/utui/thumbs`. `state.json` also remembers the last feed so the next launch reopens it (`f`/`w`/`l`/`S`/`H` pages by shortcut; searches and channels by source).

## Theme (pywal)

Utui reads `~/.cache/wal/colors.json` at startup:

| Interface role | pywal color |
| --- | --- |
| Background | `color0` / `special.background` |
| Text | `color7`, then `color15`, then `special.foreground` |
| Accent (keys, spinner, prompts) | `color4`, then `color6` |
| Success / Warning / Error | `color2` / `color3` / `color1` |
| Dim text | `color8` |

No file, no palette → your terminal's own defaults. `WAL_COLORS_FILE` overrides the path for testing.

## Account sync (real likes and Watch Later)

With `cookies_browser` set and `account_sync = true`, the `L` and `W` keys act on your actual YouTube account, not just a local list:

- `L` likes the video; `L` again unlikes it.
- `W` adds the video to Watch Later; `W` again removes it.
- `w` opens the Watch Later queue from your account. On that page `W` removes the item.

How it works: Utui ships a small embedded helper that reuses `yt-dlp`'s cookie extraction (every browser and keyring yt-dlp supports just works), signs requests to YouTube's web API with the same `SAPISIDHASH` scheme the web client uses, and calls the internal like/Watch Later endpoints. The helper is materialized to `~/.cache/utui/sync.py`, so you can inspect exactly what gets sent, and each action prints one JSON result. The local store mirrors the account state so list markers stay accurate; if the account write fails, the mirror rolls back.

Notes:

- YouTube does not let Utui read your current like state cheaply, so the first `L` on a video you already liked elsewhere will unlike it. Open the Liked page to see the truth.
- Without `cookies_browser`, or with `account_sync = false`, `L` and `W` toggle local mirrors only (the `S` Saved-list equivalent lives locally) and a toast says so.
- Feeds for Watch Later and Liked videos remain read-through yt-dlp. After a successful sync on those pages, Utui refreshes the list automatically.

## How it compares to yt-x

Utui is an independent Rust implementation inspired by the `yt-x` workflow. The core interaction model — search, feeds, channels, playback, downloads, and local history — is deliberately similar.

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

MIT. Utui is not affiliated with YouTube, yt-dlp, mpv, or the yt-x project.
