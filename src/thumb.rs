//! Thumbnail fetching, caching, and terminal image protocol detection.
//!
//! Thumbnails are downloaded once into the cache directory and decoded into an
//! `image::DynamicImage`. Rendering is handled by `ratatui-image`, which picks
//! the best available graphics protocol (kitty, sixel, iTerm2) and falls back
//! to unicode half-blocks on terminals without one.

use anyhow::{Context, Result};
use image::DynamicImage;
use ratatui_image::picker::Picker;
use std::path::Path;

/// Build a picker by querying the terminal, falling back to half-blocks when
/// no graphics protocol or font size can be detected.
pub fn picker() -> Picker {
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}

/// A picker that always uses unicode half-blocks. Used by tests and any
/// headless path that cannot query the terminal.
#[cfg(test)]
pub fn halfblocks_picker() -> Picker {
    Picker::halfblocks()
}

/// Thumbnail cache files (one per video) stay bounded too — beyond this
/// count the oldest images are dropped on each write.
const MAX_THUMB_CACHE_FILES: usize = 200;

/// Load a thumbnail from the on-disk cache, downloading and decoding it on a
/// cache miss.
pub async fn load(url: &str, cache_path: &Path) -> Result<DynamicImage> {
    let bytes = match tokio::fs::read(cache_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let bytes = download(url).await?;
            if let Some(parent) = cache_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::write(cache_path, &bytes).await;
            if let Some(parent) = cache_path.parent() {
                crate::util::prune_cache_dir(parent, MAX_THUMB_CACHE_FILES);
            }
            bytes
        }
    };

    tokio::task::spawn_blocking(move || image::load_from_memory(&bytes))
        .await
        .context("thumbnail decoding task failed")?
        .context("could not decode thumbnail")
}

/// One HTTP client for every thumbnail. The first download builds it, later
/// ones reuse its connection pool — no new socket, TLS session, or pool per
/// image, which keeps both memory and handshake churn flat.
fn http_client() -> Result<&'static reqwest::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::Client::builder()
        .user_agent(concat!("utui/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("could not build HTTP client")?;
    Ok(CLIENT.get_or_init(|| built))
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let client = http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not fetch thumbnail {url}"))?
        .error_for_status()
        .with_context(|| format!("thumbnail request failed for {url}"))?;
    let bytes = response
        .bytes()
        .await
        .context("could not read thumbnail body")?;
    Ok(bytes.to_vec())
}
