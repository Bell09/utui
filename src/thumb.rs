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

/// Every thumbnail is normalized to this pixel size. Feeds serve different
/// artwork sizes (the home feed and playlist entries do not agree), so
/// without this the details pane would draw them at different sizes.
const THUMB_WIDTH: u32 = 480;
const THUMB_HEIGHT: u32 = 270;

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

    tokio::task::spawn_blocking(move || {
        image::load_from_memory(&bytes)
            .map(normalize)
            .context("could not decode thumbnail")
    })
    .await
    .context("thumbnail decoding task failed")?
}

/// Crop an image to a 16:9 box and scale it to the fixed thumbnail size, so
/// every feed draws its artwork at exactly the same dimensions.
fn normalize(image: DynamicImage) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return image;
    }
    // Centre-crop to 16:9, then scale to the fixed size.
    let target = THUMB_WIDTH as f64 / THUMB_HEIGHT as f64;
    let current = width as f64 / height as f64;
    let (crop_width, crop_height) = if current > target {
        let w = (height as f64 * target).round() as u32;
        (w.min(width), height)
    } else {
        let h = (width as f64 / target).round() as u32;
        (width, h.min(height))
    };
    let x = (width - crop_width) / 2;
    let y = (height - crop_height) / 2;
    image.crop_imm(x, y, crop_width, crop_height).resize_exact(
        THUMB_WIDTH,
        THUMB_HEIGHT,
        image::imageops::FilterType::Triangle,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn image(width: u32, height: u32) -> DynamicImage {
        image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            width,
            height,
            Rgba([20u8, 40, 60, 255]),
        ))
    }

    #[test]
    fn normalize_scales_every_source_to_the_same_size() {
        // Feeds serve different artwork sizes; all must land on one box.
        for (width, height) in [(480u32, 270u32), (720, 404), (336, 188), (640, 480)] {
            let out = normalize(image(width, height));
            assert_eq!(out.width(), THUMB_WIDTH, "width for {width}x{height}");
            assert_eq!(out.height(), THUMB_HEIGHT, "height for {width}x{height}");
        }
    }

    #[test]
    fn normalize_leaves_degenerate_images_alone() {
        let out = normalize(image(0, 0));
        assert_eq!(out.width(), 0);
        assert_eq!(out.height(), 0);
    }
}
