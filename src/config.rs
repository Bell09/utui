use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Every persistent location Utui uses. Keeping them together makes tests and
/// alternate XDG layouts straightforward.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("io", "utui", "utui")
            .context("could not determine XDG config/data directories")?;
        let paths = Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        };
        paths.ensure()?;
        Ok(paths)
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.config_dir, &self.data_dir, &self.cache_dir] {
            fs::create_dir_all(dir)
                .with_context(|| format!("could not create directory {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    pub fn feed_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("feeds")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Player executable used for both video and audio playback.
    pub player: String,
    /// Extra arguments passed before the media URL for video playback.
    pub video_args: Vec<String>,
    /// Extra arguments passed before the media URL for audio-only playback.
    pub audio_args: Vec<String>,
    /// Number of entries requested from yt-dlp.
    pub results: usize,
    /// Where downloaded media is stored.
    pub download_dir: PathBuf,
    /// Optional browser name passed to `yt-dlp --cookies-from-browser`.
    pub cookies_browser: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let download_dir = directories::UserDirs::new()
            .map(|dirs| dirs.home_dir().join("Videos").join("utui"))
            .unwrap_or_else(|| PathBuf::from("downloads"));

        Self {
            player: "mpv".to_string(),
            video_args: vec![
                "--no-terminal".to_string(),
                "--force-window=yes".to_string(),
                "--ytdl-format=bestvideo*+bestaudio/best".to_string(),
            ],
            audio_args: vec![
                "--no-terminal".to_string(),
                "--no-video".to_string(),
                "--ytdl-format=bestaudio/best".to_string(),
            ],
            results: 50,
            download_dir,
            cookies_browser: None,
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.config_file();
        if !path.exists() {
            let config = Self::default();
            config.save(paths)?;
            return Ok(config);
        }

        let text = fs::read_to_string(&path)
            .with_context(|| format!("could not read config {}", path.display()))?;
        let mut config: Self =
            toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?;
        config.normalize();
        Ok(config)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let body = toml::to_string_pretty(self).context("could not serialize config")?;
        let text = format!(
            "# Utui configuration\n\
             # See README.md for details. Restart or press `r` in Settings to reload.\n\n{body}"
        );
        fs::write(paths.config_file(), text)
            .with_context(|| format!("could not write {}", paths.config_file().display()))
    }

    pub fn normalize(&mut self) {
        self.results = self.results.clamp(1, 500);
        if self.player.trim().is_empty() {
            self.player = "mpv".to_string();
        }
        self.cookies_browser = self
            .cookies_browser
            .take()
            .map(|browser| browser.trim().to_string())
            .filter(|browser| !browser.is_empty());
        if self.download_dir.as_os_str().is_empty() {
            self.download_dir = Self::default().download_dir;
        }
    }

    pub fn download_video_dir(&self) -> PathBuf {
        self.download_dir.join("video")
    }

    pub fn download_audio_dir(&self) -> PathBuf {
        self.download_dir.join("audio")
    }
}

/// Make sure a directory used for an external command exists.
pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("could not create directory {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_empty_values() {
        let mut config = Config {
            player: "  ".into(),
            results: 900,
            cookies_browser: Some("  ".into()),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.player, "mpv");
        assert_eq!(config.results, 500);
        assert!(config.cookies_browser.is_none());
    }
}
