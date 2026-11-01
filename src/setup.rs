//! `utui setup` — configure YouTube sign-in from the terminal.
//!
//! Authentication in Utui is cookie-based: yt-dlp reads the YouTube session
//! straight out of a browser cookie jar (`--cookies-from-browser`), and the
//! account-sync helper rides the same jar. This command picks that browser,
//! verifies the session (an offline cookie check, then optionally a live
//! Watch Later fetch), and writes `cookies_browser` / `account_sync` to
//! `config.toml`. Utui itself never sees or stores a cookie.

use anyhow::{bail, Context, Result};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::config::{Config, Paths};
use crate::sync::{self, Session};
use crate::util;
use crate::yt;

/// What the command line asked setup to do.
pub struct Options {
    /// Configure non-interactively with this cookie browser (`--browser`).
    pub browser: Option<String>,
    /// Print the current sign-in status instead of changing anything.
    pub status: bool,
    /// Skip the live Watch Later verification fetch.
    pub no_probe: bool,
    /// Account sync override; `None` keeps the current value (or asks).
    pub account_sync: Option<bool>,
}

/// Where the interactive flow sends people who still need to sign in.
const SIGNIN_URL: &str = "https://www.youtube.com/signin";

/// One cookie source `yt-dlp --cookies-from-browser` understands, plus how
/// to notice it on this machine: any listed binary on PATH or any of the
/// listed profile directories counting as "installed".
struct Candidate {
    /// The value yt-dlp accepts (before any `::profile` suffix).
    spec: &'static str,
    display: &'static str,
    bins: &'static [&'static str],
    /// Home-relative profile directories.
    dirs: &'static [&'static str],
}

/// The nine browsers yt-dlp reads cookies from (its own supported set).
const CANDIDATES: &[Candidate] = &[
    Candidate {
        spec: "firefox",
        display: "Firefox",
        bins: &["firefox", "firefox-esr"],
        dirs: &[".mozilla/firefox"],
    },
    Candidate {
        spec: "chrome",
        display: "Google Chrome",
        bins: &["google-chrome", "google-chrome-stable", "chrome"],
        dirs: &[".config/google-chrome"],
    },
    Candidate {
        spec: "chromium",
        display: "Chromium",
        bins: &["chromium", "chromium-browser"],
        dirs: &[".config/chromium"],
    },
    Candidate {
        spec: "brave",
        display: "Brave",
        bins: &["brave-browser", "brave"],
        dirs: &[".config/BraveSoftware/Brave-Browser"],
    },
    Candidate {
        spec: "edge",
        display: "Microsoft Edge",
        bins: &["microsoft-edge", "microsoft-edge-stable"],
        dirs: &[".config/microsoft-edge"],
    },
    Candidate {
        spec: "opera",
        display: "Opera",
        bins: &["opera"],
        dirs: &[".config/opera"],
    },
    Candidate {
        spec: "vivaldi",
        display: "Vivaldi",
        bins: &["vivaldi-stable", "vivaldi"],
        dirs: &[".config/vivaldi"],
    },
    Candidate {
        spec: "whale",
        display: "Whale",
        bins: &["whale"],
        dirs: &[".config/whale"],
    },
    // Safari has no CLI and ships with macOS; kept in the table so the
    // accepted set matches yt-dlp's, but only offered where it exists.
    Candidate {
        spec: "safari",
        display: "Safari",
        bins: &[],
        dirs: &["Library/Application Support/com.apple.Safari"],
    },
];

/// A candidate resolved against this machine.
struct Found {
    spec: &'static str,
    display: &'static str,
    found: bool,
    note: String,
}

pub async fn run(paths: &Paths, mut config: Config, options: Options) -> Result<()> {
    let script = paths.cache_dir.join("sync.py");
    if options.status {
        return print_status(paths, &config, &script).await;
    }

    let interactive = io::stdin().is_terminal();
    println!("utui setup — YouTube sign-in");
    println!();

    let browser = match options.browser.as_deref() {
        Some(raw) => {
            let spec = normalize_spec(raw);
            validate_spec(&spec)?;
            spec
        }
        None => {
            if !interactive {
                bail!(
                    "`utui setup` prompts for a browser when run interactively; pass \
                     `--browser <name>` to configure directly, or `--status` to check."
                );
            }
            prompt_browser(&config)?
        }
    };
    println!("  browser   {browser}");

    // Offline session check: classify without touching the network. Failures
    // to read the jar become `Unknown`, never a false "signed out".
    sync::prepare_script(&script)?;
    let mut session = sync::session_check(&browser, &script)
        .await
        .unwrap_or_else(|error| Session::Unknown(error.to_string()));
    loop {
        match &session {
            Session::SignedIn => {
                println!("  session   ✓ signed in to YouTube ({browser})");
                break;
            }
            Session::Unknown(detail) => {
                println!("  session   ? {detail}");
                break;
            }
            Session::SignedOut(detail) => {
                println!("  session   ✗ {detail}");
                if !interactive {
                    break;
                }
                if !ask_yes_no("Open the YouTube sign-in page in your browser?", true)? {
                    break;
                }
                open_signin();
                read_line("Press Enter once you are signed in to re-check ")?;
                session = sync::session_check(&browser, &script)
                    .await
                    .unwrap_or_else(|error| Session::Unknown(error.to_string()));
            }
        }
    }

    let signed_out_detail: Option<String> = match &session {
        Session::SignedOut(detail) => Some(detail.clone()),
        _ => None,
    };
    // Interactive: an empty jar gets an explicit choice before anything is
    // written. Non-interactive: `--browser` already is that choice.
    if signed_out_detail.is_some()
        && interactive
        && !ask_yes_no(
            &format!("Save cookies_browser = \"{browser}\" anyway?"),
            false,
        )?
    {
        println!("Nothing saved.");
        return Ok(());
    }

    // Live verification: Watch Later only exists for a signed-in session, so
    // a successful fetch proves the whole cookie path end-to-end.
    let run_probe = if options.no_probe {
        false
    } else if interactive {
        ask_yes_no(
            "Verify with a live Watch Later fetch (uses the network)?",
            signed_out_detail.is_none(),
        )?
    } else {
        signed_out_detail.is_none()
    };
    let mut probe_error = None;
    if run_probe {
        match probe(&browser).await {
            Ok(count) => println!("  probe     ✓ Watch Later returned {count} videos (live)"),
            Err(error) => {
                println!("  probe     ✗ {error}");
                probe_error = Some(error);
            }
        }
    }

    let account_sync = match options.account_sync {
        Some(value) => value,
        None if interactive => ask_yes_no(
            "Sync likes and Watch Later to your account (l / w keys)?",
            config.account_sync,
        )?,
        None => config.account_sync,
    };

    config.cookies_browser = Some(browser.clone());
    config.account_sync = account_sync;
    config.normalize();
    config.save(paths)?;
    println!();
    println!("Saved {}", paths.config_file().display());
    println!("  cookies_browser = \"{browser}\"");
    println!(
        "  account_sync    = {}",
        if account_sync { "true" } else { "false" }
    );

    // Non-interactive callers get a failing exit code when verification did
    // not go through, after the requested configuration was still written.
    if !interactive {
        if let Some(error) = probe_error {
            bail!("saved, but the live check failed: {error}");
        }
        if let Some(detail) = signed_out_detail {
            bail!("saved, but {browser} has no signed-in YouTube session: {detail}");
        }
    }

    println!();
    println!("Restart Utui to pick it up. Cookie feeds unlock right away:");
    println!("  1. My Feeds · 2. Watch Later · 3. Liked videos · f. Subscriptions");
    if account_sync {
        println!("  l / w like or queue any video straight to your account");
    }
    Ok(())
}

/// `utui setup --status`: everything known without changing anything.
async fn print_status(paths: &Paths, config: &Config, script: &Path) -> Result<()> {
    println!("utui — YouTube sign-in status");
    println!("  config file     {}", paths.config_file().display());
    match &config.cookies_browser {
        Some(browser) => println!("  cookies_browser {browser}"),
        None => println!("  cookies_browser not set  →  run: utui setup"),
    }
    println!(
        "  account_sync    {}",
        if config.account_sync { "on" } else { "off" }
    );
    if let Some(browser) = &config.cookies_browser {
        sync::prepare_script(script)?;
        match sync::session_check(browser, script)
            .await
            .unwrap_or_else(|error| Session::Unknown(error.to_string()))
        {
            Session::SignedIn => println!("  session         ✓ signed in to YouTube via {browser}"),
            Session::SignedOut(detail) => {
                println!("  session         ✗ {detail}");
                println!(
                    "                  sign in to YouTube in {browser}, then rerun utui setup"
                );
            }
            Session::Unknown(detail) => println!("  session         ? {detail}"),
        }
    }
    Ok(())
}

/// One live, account-scoped fetch. Returns how many entries came back.
async fn probe(browser: &str) -> Result<usize> {
    let mut spec = yt::FetchSpec::new(0, "https://www.youtube.com/playlist?list=WL", "Watch Later");
    spec.limit = 3;
    spec.cookies_browser = Some(browser.to_string());
    spec.requires_auth = true;
    // Probes never warm the feed cache: `cache_dir` stays unset.
    spec.bypass_cache = true;
    Ok(yt::fetch(&spec).await?.items.len())
}

/// The interactive browser menu: detected browsers, the current setting,
/// then a number or any spec yt-dlp accepts.
fn prompt_browser(config: &Config) -> Result<String> {
    println!("Utui signs in through your browser's cookie jar — yt-dlp reads it,");
    println!("Utui never stores cookies itself.");
    println!();
    println!("Pick the browser you use for YouTube:");

    let all = detect(&home_dir(), &env_path());
    let missing: Vec<&str> = all.iter().filter(|f| !f.found).map(|f| f.spec).collect();
    let present: Vec<Found> = all.into_iter().filter(|f| f.found).collect();

    let default = if present.is_empty() {
        None
    } else {
        config
            .cookies_browser
            .as_deref()
            .and_then(|current| present.iter().position(|f| f.spec == current))
            .map(|index| index + 1)
            .or(Some(1))
    };
    if let Some(current) = &config.cookies_browser {
        println!("  current setting: {current}");
    }
    for (index, candidate) in present.iter().enumerate() {
        println!(
            "  {}) {:<20} {}",
            index + 1,
            candidate.display,
            candidate.note
        );
    }
    if !missing.is_empty() {
        println!("  not found: {}", missing.join(", "));
    }
    if present.is_empty() {
        println!("  no supported browser detected — type what yt-dlp should read,");
        println!("  e.g. firefox, chrome, or firefox::/path/to/profile");
    } else {
        println!("  (or type any spec yt-dlp accepts, e.g. firefox::/path/to/profile)");
    }

    loop {
        let prompt = match default {
            Some(number) => format!("Browser [{number}]: "),
            None => "Browser: ".to_string(),
        };
        let answer = read_line(&prompt)?;
        match resolve_choice(&answer, &present, default) {
            Ok(spec) => return Ok(spec),
            Err(error) => println!("  {error}"),
        }
    }
}

/// Menu input: a displayed number, or a browser spec (validated). Empty
/// input takes the highlighted default when there is one.
fn resolve_choice(input: &str, present: &[Found], default: Option<usize>) -> Result<String> {
    let input = input.trim();
    if input.is_empty() {
        if let Some(found) = default.and_then(|number| present.get(number - 1)) {
            return Ok(found.spec.to_string());
        }
        bail!("type a browser name, or pick a listed number");
    }
    if let Ok(index) = input.parse::<usize>() {
        let found = index.checked_sub(1).and_then(|i| present.get(i));
        let Some(found) = found else {
            bail!("pick a number 1..={} or type a browser name", present.len());
        };
        return Ok(found.spec.to_string());
    }
    let spec = normalize_spec(input);
    validate_spec(&spec)?;
    Ok(spec)
}

/// Trim the input and lowercase the browser segment (`Chrome::/p` → `chrome::/p`).
fn normalize_spec(raw: &str) -> String {
    let spec = raw.trim();
    match spec.split_once("::") {
        Some((head, rest)) => format!("{}::{rest}", head.to_ascii_lowercase()),
        None => spec.to_ascii_lowercase(),
    }
}

/// Refuse specs whose browser segment yt-dlp does not know, with the
/// supported set in the error, instead of failing later inside a fetch.
fn validate_spec(spec: &str) -> Result<()> {
    let name = spec
        .split("::")
        .next()
        .unwrap_or_default()
        .split('+')
        .next()
        .unwrap_or_default();
    if name.is_empty() {
        bail!("empty browser name — expected something like `firefox` or `chrome`");
    }
    if CANDIDATES.iter().any(|candidate| candidate.spec == name) {
        return Ok(());
    }
    bail!(
        "yt-dlp cannot read cookies from {name:?}; supported: {}",
        CANDIDATES
            .iter()
            .map(|candidate| candidate.spec)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Resolve every candidate against a home directory and a PATH string so
/// detection is deterministic in tests.
fn detect(home: &Path, path_var: &str) -> Vec<Found> {
    let binary_dirs: Vec<&Path> = path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(Path::new)
        .collect();
    CANDIDATES
        .iter()
        .filter(|candidate| candidate.spec != "safari" || cfg!(target_os = "macos"))
        .map(|candidate| {
            let bin = candidate
                .bins
                .iter()
                .find(|name| binary_dirs.iter().any(|dir| dir.join(name).is_file()));
            let has_profile = candidate.dirs.iter().any(|rel| home.join(rel).exists());
            let note = match (bin, has_profile) {
                (Some(bin), true) => format!("{bin} + profile"),
                (Some(bin), false) => format!("{bin} (no profile yet)"),
                (None, true) => "profile".to_string(),
                (None, false) => "not found".to_string(),
            };
            Found {
                spec: candidate.spec,
                display: candidate.display,
                found: bin.is_some() || has_profile,
                note,
            }
        })
        .collect()
}

fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn env_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

fn open_signin() {
    if let Err(error) = util::open_external(SIGNIN_URL) {
        println!("  (could not open a browser: {error} — open {SIGNIN_URL} yourself)");
    }
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let read = io::stdin()
        .read_line(&mut line)
        .context("could not read input")?;
    if read == 0 {
        bail!("aborted (end of input)");
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn ask_yes_no(question: &str, default: bool) -> Result<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let answer = read_line(&format!("{question} {hint} "))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("  please answer y or n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn choice_accepts_numbers_defaults_and_typed_specs() {
        let present = vec![
            Found {
                spec: "firefox",
                display: "Firefox",
                found: true,
                note: String::new(),
            },
            Found {
                spec: "chrome",
                display: "Google Chrome",
                found: true,
                note: String::new(),
            },
        ];
        assert_eq!(resolve_choice("2", &present, None).unwrap(), "chrome");
        assert_eq!(resolve_choice("", &present, Some(1)).unwrap(), "firefox");
        assert_eq!(
            resolve_choice("Firefox", &present, None).unwrap(),
            "firefox"
        );
        // Typed specs may reference a browser the detector never saw.
        assert_eq!(
            resolve_choice("chromium", &present, None).unwrap(),
            "chromium"
        );
        assert!(resolve_choice("0", &present, None).is_err());
        assert!(resolve_choice("9", &present, None).is_err());
        assert!(resolve_choice("", &present, None).is_err());
        assert!(resolve_choice("netscape", &present, None).is_err());
    }

    #[test]
    fn spec_validation_matches_yt_dlp() {
        assert!(validate_spec("brave").is_ok());
        assert!(validate_spec("firefox::/home/u/.librewolf/abc").is_ok());
        assert!(validate_spec("brave+yand2fz:").is_ok());
        assert!(validate_spec("").is_err());
        assert!(validate_spec("::").is_err());
        assert!(validate_spec("netscape").is_err());
        assert_eq!(normalize_spec("  Chrome::/tmp/Prof "), "chrome::/tmp/Prof");
    }

    #[test]
    fn detects_browsers_from_binaries_and_profiles() {
        let root = std::env::temp_dir().join(format!("utui-setup-{}", std::process::id()));
        let home = root.join("home");
        let bin = root.join("bin");
        fs::create_dir_all(home.join(".config/chromium")).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("brave-browser"), b"").unwrap();

        let found = detect(&home, bin.to_str().unwrap());
        let spec = |name: &str| {
            found
                .iter()
                .find(|candidate| candidate.spec == name)
                .unwrap_or_else(|| panic!("{name} missing from detection"))
        };
        assert!(spec("chromium").found, "profile-only browsers count");
        assert!(spec("brave").found, "PATH binaries count");
        assert!(!spec("firefox").found, "no profile and no binary here");
        // Safari is macOS-only and must not leak into other platforms' menus.
        assert!(cfg!(target_os = "macos") || !found.iter().any(|c| c.spec == "safari"));

        fs::remove_dir_all(&root).ok();
    }
}
