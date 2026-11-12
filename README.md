# Utui

**Minimalist terminal YouTube browser** written in Rust.

One native binary. No shell scripts, no `fzf`, no `jq`.  
`yt-dlp` extracts · `mpv` plays · ratatui keeps it fast and keyboard-first.

---

## Features

- **Native TUI** — clean terminal palette + one accent color
- **Fast search** — single `yt-dlp` call, 10-minute cache
- **Thumbnails** — kitty / sixel / iTerm2 (unicode fallback)
- **Filters** — `:today` `:4k` `:live` `:newest` and more
- **Feeds** — My Feeds, Subscriptions, Watch Later, Liked (cookie-backed)
- **Playback** — video or audio-only via `mpv`
- **Downloads** — quality picker or MP3, non-blocking
- **Queue** — build a playlist on the fly (`X` / `Z` / `z`)
- **Library** — saved videos + history
- **Theming** — live pywal / wallust / matugen (background stays transparent)
- **Scriptable** — `--json` mode

---

## Requirements

| Tool | Purpose |
|------|---------|
| `yt-dlp` | Search, feeds, media |
| `mpv` | Playback |
| `ffmpeg` | Downloads |
| clipboard tool | Optional (`y` to copy URL) |

Rust 1.86+ to build.

---

## Install

```bash
git clone https://github.com/Bell09/utui
cd utui
cargo build --release
install -Dm755 target/release/utui ~/.local/bin/utui
```

**Arch Linux (AUR)**

```bash
paru -S utui   # or yay -S utui
```

---

## Usage

```bash
utui                          # home screen
utui setup                    # configure cookies / account
utui --search "rust"          # search immediately
utui --url "https://..."      # open any URL
utui --search "lofi" --json   # headless / scripting
```

### Common flags

| Flag | Description |
|------|-------------|
| `-s, --search <QUERY>` | Start with search |
| `-u, --url <URL>` | Open URL |
| `--json` | Machine-readable output |
| `-n, --limit <N>` | Result count |
| `--clear-cache` | Wipe feed cache |

---

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `/` | Search |
| `m` | Action menu |
| `?` | Help |
| `q` | Quit |

### Navigation & Playback

| Key | Action |
|-----|--------|
| `j` / `k` · `↓` / `↑` | Move |
| `Enter` / `Space` | Play video |
| `A` | Play audio only |
| `d` | Download (quality picker) |
| `D` | Download MP3 |
| `c` | Open channel |
| `y` | Copy URL |
| `o` | Open in browser |
| `s` | Save / unsave |
| `l` | Like / unlike |
| `w` | Watch Later |
| `X` / `Z` | Queue next / end |
| `z` | Open queue |
| `1`–`5` | Jump feeds |
| `R` | Refresh |
| `b` / `Esc` | Back |

### Queue page (`z`)

| Key | Action |
|-----|--------|
| `J` / `K` | Reorder |
| `d` | Remove |
| `Enter` | Play from here |

---

## Configuration

Created on first run at `~/.config/utui/config.toml`:

```toml
player = "mpv"
results = 50
download_dir = "~/Videos/utui"

# cookies_browser = "firefox"   # for personalized feeds
# account_sync = true           # real likes / Watch Later
```

State: `~/.local/share/utui/state.json`  
Cache: `~/.cache/utui/`

Run `utui setup` for interactive cookie configuration.

---

## Theme

Detects pywal, wallust, or matugen automatically.  
Live reloads when you change wallpaper.  
Background is never painted — terminal transparency is preserved.

Without a theme tool → terminal defaults + cyan accent.

---

## License

MIT

Not affiliated with YouTube, yt-dlp, or mpv.
