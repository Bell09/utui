mod app;
mod config;
mod setup;
mod store;
mod sync;
mod theme;
mod thumb;
mod ui;
mod util;
mod yt;

use anyhow::{bail, Context, Result};
use app::{App, Effect, Msg};
use clap::{Parser, Subcommand};
use config::Config;
use crossterm::event::{self, Event};
use serde::Serialize;
use std::io::IsTerminal;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};
use store::Store;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Parser, Debug)]
#[command(
    name = "utui",
    version,
    about = "A blazing-fast terminal YouTube browser",
    long_about = "Browse YouTube and anything yt-dlp supports from a native Rust TUI.\n\
                  Playback uses mpv, downloads use yt-dlp, and cookie-backed feeds are opt-in.\n\
                  Run `utui setup` to configure YouTube sign-in."
)]
struct Cli {
    /// Search YouTube and open the results in the TUI.
    #[arg(short, long, conflicts_with = "url")]
    search: Option<String>,

    /// Open a video, playlist, channel, or any yt-dlp URL in the TUI.
    #[arg(short, long)]
    url: Option<String>,

    /// Print JSON instead of launching the TUI. Requires --search or --url.
    #[arg(long)]
    json: bool,

    /// Number of entries to request. Overrides config.toml.
    #[arg(short = 'n', long)]
    limit: Option<usize>,

    /// Browser to read YouTube cookies from, e.g. firefox or chrome.
    #[arg(long)]
    cookies_from_browser: Option<String>,

    /// Ignore the on-disk feed cache for this run.
    #[arg(long)]
    no_cache: bool,

    /// Clear the feed cache before starting.
    #[arg(long)]
    clear_cache: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Configure YouTube sign-in: scans browsers for sessions, then verifies.
    Setup {
        /// Configure without prompting, e.g. `utui setup --browser firefox`.
        #[arg(long)]
        browser: Option<String>,

        /// Print the current sign-in status and exit.
        #[arg(long, conflicts_with_all = ["browser", "account_sync"])]
        status: bool,

        /// Skip the live Watch Later verification fetch.
        #[arg(long)]
        no_probe: bool,

        /// Turn account sync on or off (default: keep the current value,
        /// or ask when running interactively).
        #[arg(long, value_name = "BOOL")]
        account_sync: Option<bool>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = config::Paths::discover()?;
    let mut config = Config::load(&paths)?;

    if let Some(browser) = cli.cookies_from_browser.as_deref() {
        let browser = browser.trim();
        config.cookies_browser = (!browser.is_empty()).then(|| browser.to_string());
    }
    if let Some(limit) = cli.limit {
        config.results = limit.clamp(1, 500);
    }

    // `utui setup` configures sign-in in place: no TUI, no feed, and it
    // takes precedence over the launching flags above.
    if let Some(Commands::Setup {
        browser,
        status,
        no_probe,
        account_sync,
    }) = &cli.command
    {
        let options = setup::Options {
            browser: browser.clone(),
            status: *status,
            no_probe: *no_probe,
            account_sync: *account_sync,
        };
        return setup::run(&paths, config, options).await;
    }

    if cli.clear_cache {
        yt::clear_cache(&paths.cache_dir)?;
        if cli.search.is_none() && cli.url.is_none() {
            println!("Feed cache cleared.");
            return Ok(());
        }
    }

    if cli.json {
        return run_json(&cli, &config, &paths).await;
    }

    if !std::io::stdout().is_terminal() {
        bail!(
            "Utui needs a terminal for the TUI. Use --json for scriptable output, \
             or run it from an interactive shell."
        );
    }

    let store = Store::load(&paths.state_file())?;
    let mut app = App::new(paths, config, store);

    if let Some(query) = cli.search.as_deref() {
        if let Err(error) = app.start_search(query) {
            app.toast_error(error.to_string());
        }
    } else if let Some(url) = cli.url.as_deref() {
        if let Err(error) = app.start_url(url) {
            app.toast_error(error.to_string());
        }
    }

    run_tui(&mut app).await
}

async fn run_json(cli: &Cli, config: &Config, paths: &config::Paths) -> Result<()> {
    let (url, label) = if let Some(query) = cli.search.as_deref() {
        let plan = yt::parse_search(query);
        if plan.term.trim().is_empty() {
            bail!("Add a search term after the filter");
        }
        (
            yt::build_search_url(&plan.term, &plan.sp),
            format!("Search · {}", plan.term),
        )
    } else if let Some(url) = cli.url.as_deref() {
        (url.to_string(), "URL".to_string())
    } else {
        bail!("--json requires --search or --url");
    };

    let mut spec = yt::FetchSpec::new(0, url, label);
    spec.limit = config.results;
    spec.cookies_browser = config.cookies_browser.clone();
    spec.cache_dir = Some(paths.feed_cache_dir());
    spec.bypass_cache = cli.no_cache;
    let feed = yt::fetch(&spec).await?;

    #[derive(Serialize)]
    struct JsonFeed<'a> {
        title: &'a str,
        source: &'a str,
        count: usize,
        items: &'a [yt::Video],
    }

    let output = JsonFeed {
        title: &feed.title,
        source: &feed.source,
        count: feed.items.len(),
        items: &feed.items,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn run_tui(app: &mut App) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut terminal = ratatui::init();
    app.set_picker(thumb::picker());
    let mut active_fetch: Option<JoinHandle<()>> = None;
    let mut active_thumb: Option<JoinHandle<()>> = None;
    let mut active_sync: Option<JoinHandle<()>> = None;

    let result: Result<()> = async {
        loop {
            if event::poll(Duration::from_millis(35))? {
                match event::read()? {
                    Event::Key(key) => app.handle_key(key),
                    Event::Paste(text) => handle_paste(app, &text),
                    _ => {}
                }
            }

            while let Ok(message) = rx.try_recv() {
                app.handle_msg(message);
            }

            if let Some(spec) = app.take_fetch() {
                if let Some(handle) = active_fetch.take() {
                    handle.abort();
                }
                let tx = tx.clone();
                active_fetch = Some(tokio::spawn(async move {
                    let id = spec.id;
                    let result = yt::fetch(&spec).await.map_err(|error| error.to_string());
                    let _ = tx.send(Msg::Feed { id, result });
                }));
            }

            if let Some(effect) = app.take_effect() {
                match effect {
                    Effect::Play { video, audio } => {
                        suspend_tui();
                        let outcome = play_media(&app.config, &video, audio);
                        terminal = ratatui::init();
                        terminal.clear()?;
                        app.clear_thumbnail();
                        match outcome {
                            Ok(()) => app.toast_info(format!("Finished · {}", video.title)),
                            Err(error) => app.toast_error(error.to_string()),
                        }
                    }
                    Effect::Download {
                        job_id,
                        video,
                        audio,
                        height,
                    } => {
                        let config = app.config.clone();
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let outcome =
                                download_media(&config, &video, audio, height, &tx, job_id).await;
                            let (ok, message) = match outcome {
                                Ok(message) => (true, message),
                                Err(error) => (false, error.to_string()),
                            };
                            let _ = tx.send(Msg::DownloadDone {
                                id: job_id,
                                ok,
                                message,
                            });
                        });
                    }
                    Effect::Copy(value) => app.copy_url(&value),
                    Effect::OpenUrl(url) => match util::open_external(&url) {
                        Ok(()) => app.toast_success("Opened in your browser"),
                        Err(error) => app.toast_error(error.to_string()),
                    },
                    Effect::Sync { video, action } => {
                        let script = app.paths.cache_dir.join("sync.py");
                        if let Err(error) = sync::prepare_script(&script) {
                            app.toast_error(error.to_string());
                        } else {
                            if let Some(handle) = active_sync.take() {
                                handle.abort();
                            }
                            let browser = app.config.cookies_browser.clone().unwrap_or_default();
                            let tx = tx.clone();
                            active_sync = Some(tokio::spawn(async move {
                                let outcome = sync::run(&action, &browser, &script)
                                    .await
                                    .map_err(|error| error.to_string());
                                let (ok, message) = match outcome {
                                    Ok(summary) => (true, summary),
                                    Err(message) => (false, message),
                                };
                                let context = util::truncate(&video.title, 40);
                                let _ = tx.send(Msg::SyncDone {
                                    video_id: video.id.clone(),
                                    action,
                                    ok,
                                    message: format!("{context}: {message}"),
                                });
                            }));
                        }
                    }
                }
            }

            app.sync_thumbnail();
            if let Some(spec) = app.take_thumbnail() {
                if let Some(handle) = active_thumb.take() {
                    handle.abort();
                }
                let tx = tx.clone();
                active_thumb = Some(tokio::spawn(async move {
                    let result = thumb::load(&spec.url, &spec.path)
                        .await
                        .map_err(|error| error.to_string());
                    let _ = tx.send(Msg::Thumbnail {
                        id: spec.id,
                        result,
                    });
                }));
            }

            terminal.draw(|frame| ui::draw(frame, app, Instant::now()))?;

            if app.quit {
                break;
            }
        }
        Ok(())
    }
    .await;

    ratatui::restore();
    if let Err(error) = app.persist() {
        eprintln!("utui: could not save library: {error}");
    }
    result
}

fn handle_paste(app: &mut App, text: &str) {
    if let Some(app::Overlay::Search(input)) = app.overlay.as_mut() {
        input.insert_str(text);
    }
}

fn suspend_tui() {
    ratatui::restore();
}

fn play_media(config: &Config, video: &yt::Video, audio: bool) -> Result<()> {
    let args = if audio {
        &config.audio_args
    } else {
        &config.video_args
    };
    let mut command = StdCommand::new(&config.player);
    command
        .args(args)
        .arg(format!("--force-media-title={}", video.title))
        .arg("--")
        .arg(&video.url);
    let status = command
        .status()
        .with_context(|| format!("could not launch player {}", config.player))?;
    if !status.success() {
        bail!("{} exited with {status}", config.player);
    }
    Ok(())
}

/// The yt-dlp selector for a video download: the quality picker's
/// `height<=` cap (closest stream at or below it), or the default best
/// when no height was picked.
fn video_format(height: Option<u32>) -> String {
    match height {
        Some(height) => format!("bv*[height<={height}]+ba/b[height<={height}]"),
        None => "bestvideo*+bestaudio/best".to_string(),
    }
}

async fn download_media(
    config: &Config,
    video: &yt::Video,
    audio: bool,
    height: Option<u32>,
    tx: &mpsc::UnboundedSender<Msg>,
    job_id: u64,
) -> Result<String> {
    let directory = if audio {
        config.download_audio_dir()
    } else {
        config.download_video_dir()
    };
    config::ensure_dir(&directory)?;

    let output_template = directory
        .join("%(uploader,channel,creator|Unknown)s")
        .join("%(title).180B [%(id)s].%(ext)s");

    let mut command = Command::new("yt-dlp");
    command
        .arg("--newline")
        .arg("--no-colors")
        .arg("--no-warnings")
        .arg("--ignore-errors")
        .arg("--no-playlist")
        .arg("--embed-metadata")
        .arg("--output")
        .arg(output_template);
    if audio {
        command
            .arg("--format")
            .arg("bestaudio/best")
            .arg("--extract-audio")
            .arg("--audio-format")
            .arg("mp3")
            .arg("--audio-quality")
            .arg("0")
            .arg("--embed-thumbnail");
    } else {
        command
            .arg("--format")
            .arg(video_format(height))
            .arg("--merge-output-format")
            .arg("mp4");
    }
    if let Some(browser) = &config.cookies_browser {
        command.arg("--cookies-from-browser").arg(browser);
    }
    command
        .arg("--")
        .arg(&video.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .context("could not launch yt-dlp (is it installed and on PATH?)")?;
    let stdout = child
        .stdout
        .take()
        .context("could not capture yt-dlp output")?;
    let stderr = child
        .stderr
        .take()
        .context("could not capture yt-dlp errors")?;

    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut last = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if !line.is_empty() {
                last = line;
            }
        }
        last
    });

    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(percent) = util::parse_percent(&line) {
            let _ = tx.send(Msg::DownloadProgress {
                id: job_id,
                percent,
            });
        }
    }

    let status = child.wait().await.context("could not wait for yt-dlp")?;
    let last_error = stderr_task.await.unwrap_or_default();
    if status.success() {
        Ok(format!("Downloaded to {}", directory.display()))
    } else {
        bail!(
            "Download failed{}",
            if last_error.is_empty() {
                format!(" ({status})")
            } else {
                format!(": {last_error}")
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::video_format;

    #[test]
    fn quality_cap_builds_selector() {
        assert_eq!(
            video_format(Some(720)),
            "bv*[height<=720]+ba/b[height<=720]"
        );
        assert_eq!(
            video_format(Some(160)),
            "bv*[height<=160]+ba/b[height<=160]"
        );
        assert_eq!(video_format(None), "bestvideo*+bestaudio/best");
    }
}
