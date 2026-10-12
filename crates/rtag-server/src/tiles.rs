//! Tile pyramids for the annotation workspace. The first tile request for an image decodes it once
//! and writes every tile of every level to the cache; later requests read the cached files.
//! Viewing tiles are JPEG; analysis tiles (for the fuzzy fill and edge snapping, ADR 0002) are
//! the same pyramid as lossless PNG, generated the first time one is asked for.

use std::{
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    extract::{Path as UrlPath, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Pixel, codecs::jpeg::JpegEncoder, imageops::FilterType};
use rtag_core::pyramid::{Pyramid, TILE_SIZE};
use sqlx::Row;

use crate::{AppState, error::ApiError, thumbnails::open_image, write_atomically};

const JPEG_QUALITY: u8 = 90;
/// Written last, so its presence means every tile of the pyramid is in the cache.
const COMPLETE_MARKER: &str = "complete";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/images/{id}/tiles/{level}/{x}/{y}", get(|state, url| tile(state, url, Kind::View)))
        .route("/api/images/{id}/analysis/{level}/{x}/{y}", get(|state, url| tile(state, url, Kind::Analysis)))
}

/// Which pyramid of an image.
#[derive(Clone, Copy)]
enum Kind {
    /// JPEG tiles for looking at.
    View,
    /// Lossless PNG tiles whose pixels are analysed; JPEG block edges would mislead.
    Analysis,
}

impl Kind {
    fn folder(self) -> &'static str {
        match self {
            Kind::View => "original",
            Kind::Analysis => "analysis",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Kind::View => "jpg",
            Kind::Analysis => "png",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Kind::View => "image/jpeg",
            Kind::Analysis => "image/png",
        }
    }
}

/// One lock per image hash, so concurrent first requests generate a pyramid only once.
#[derive(Clone, Default)]
pub struct GenerationLocks(Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>);

impl GenerationLocks {
    fn for_key(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.0.lock().expect("lock map poisoned");
        // Forget locks nobody else holds, so the map does not grow with every image ever viewed.
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks.entry(key.to_owned()).or_default().clone()
    }
}

async fn tile(
    State(state): State<AppState>,
    UrlPath((id, level, x, y)): UrlPath<(i64, u32, u32, u32)>,
    kind: Kind,
) -> Result<impl IntoResponse, ApiError> {
    let row = sqlx::query("SELECT path, hash, width, height FROM images WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    let pyramid = Pyramid::new(row.get("width"), row.get("height"));
    pyramid.tile_bounds(level, x, y).ok_or(ApiError::NotFound)?;

    let hash: String = row.get("hash");
    // Named by content hash, so identical images share one pyramid. Enhancement filters will get
    // their own folders next to `original` and `analysis`.
    let dir = state.cache_dir.join("tiles").join(&hash).join(kind.folder());
    if !dir.join(COMPLETE_MARKER).exists() {
        let lock = state.pyramid_locks.for_key(&format!("{hash}/{}", kind.folder()));
        let _generating = lock.lock().await;
        if !dir.join(COMPLETE_MARKER).exists() {
            let source = state.images_root.join(row.get::<String, _>("path"));
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || generate(&source, &dir, pyramid, kind))
                .await
                .expect("tile generation panicked")?;
        }
    }

    let bytes = fs::read(tile_path(&dir, level, x, y, kind))
        .map_err(|err| ApiError::Internal(format!("cannot read cached tile: {err}")))?;
    // A tile of an image id never changes: its content hash is fixed at import.
    Ok(([(header::CONTENT_TYPE, kind.content_type()), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], bytes))
}

fn tile_path(dir: &Path, level: u32, x: u32, y: u32, kind: Kind) -> PathBuf {
    dir.join(level.to_string()).join(format!("{x}_{y}.{}", kind.extension()))
}

/// Decodes the source and writes all tiles of all levels, each level downscaled from the previous one.
fn generate(source: &Path, dir: &Path, pyramid: Pyramid, kind: Kind) -> Result<(), ApiError> {
    let image = open_image(source)?;
    if (image.width(), image.height()) != (pyramid.width, pyramid.height) {
        return Err(ApiError::Internal(format!("{} has changed size since it was imported", source.display())));
    }
    // Grayscale sources stay single-channel: smaller tiles, same detail.
    let base = if image.color().has_color() {
        DynamicImage::ImageRgb8(image.to_rgb8())
    } else {
        DynamicImage::ImageLuma8(image.to_luma8())
    };
    drop(image);
    let mut levels = vec![base];
    for level in 1..pyramid.levels() {
        let (width, height) = pyramid.level_size(level);
        let finer = levels.last().expect("level 0 exists");
        levels.push(halve(finer, width, height));
    }

    // Encoding dominates, so spread the tiles over all cores.
    let tiles: Vec<(u32, u32, u32)> = (0..pyramid.levels())
        .flat_map(|level| {
            let (columns, rows) = pyramid.tile_grid(level);
            (0..columns).flat_map(move |x| (0..rows).map(move |y| (level, x, y)))
        })
        .collect();
    let next = AtomicUsize::new(0);
    let write_tiles = || -> Result<(), ApiError> {
        while let Some(&(level, x, y)) = tiles.get(next.fetch_add(1, Ordering::Relaxed)) {
            let bounds = pyramid.tile_bounds(level, x, y).expect("tile is inside the grid");
            let tile = levels[level as usize].crop_imm(bounds.x, bounds.y, bounds.width, bounds.height);
            write_atomically(&tile_path(dir, level, x, y, kind), &encode(&tile, kind)?)
                .map_err(|err| ApiError::Internal(format!("cannot cache tile: {err}")))?;
        }
        Ok(())
    };
    let workers = std::thread::available_parallelism().map_or(1, usize::from).min(tiles.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers).map(|_| scope.spawn(write_tiles)).collect();
        handles.into_iter().try_for_each(|handle| handle.join().expect("tile worker panicked"))
    })?;
    write_atomically(&dir.join(COMPLETE_MARKER), format!("{TILE_SIZE}\n").as_bytes())
        .map_err(|err| ApiError::Internal(format!("cannot cache tile: {err}")))
}

/// Downscales to (`width`, `height`), which is half of `image` rounded up, averaging each 2×2 block.
/// An odd last row or column is averaged with itself.
fn halve(image: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    match image {
        DynamicImage::ImageLuma8(buffer) => DynamicImage::ImageLuma8(halve_buffer(buffer, width, height)),
        DynamicImage::ImageRgb8(buffer) => DynamicImage::ImageRgb8(halve_buffer(buffer, width, height)),
        other => other.resize_exact(width, height, FilterType::Triangle),
    }
}

fn halve_buffer<P: Pixel<Subpixel = u8>>(source: &ImageBuffer<P, Vec<u8>>, width: u32, height: u32) -> ImageBuffer<P, Vec<u8>> {
    let channels = usize::from(P::CHANNEL_COUNT);
    let (source_width, source_height) = (source.width() as usize, source.height() as usize);
    let raw = source.as_raw();
    let at = |x: usize, y: usize, channel: usize| u16::from(raw[(y * source_width + x) * channels + channel]);
    let mut halved = Vec::with_capacity(width as usize * height as usize * channels);
    for y in 0..height as usize {
        let (top, bottom) = (2 * y, (2 * y + 1).min(source_height - 1));
        for x in 0..width as usize {
            let (left, right) = (2 * x, (2 * x + 1).min(source_width - 1));
            for channel in 0..channels {
                let sum = at(left, top, channel) + at(right, top, channel) + at(left, bottom, channel) + at(right, bottom, channel);
                halved.push(((sum + 2) / 4) as u8);
            }
        }
    }
    ImageBuffer::from_raw(width, height, halved).expect("buffer matches the halved size")
}

fn encode(tile: &DynamicImage, kind: Kind) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    let mut out = Cursor::new(&mut bytes);
    match kind {
        Kind::View => JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(tile),
        Kind::Analysis => tile.write_to(&mut out, ImageFormat::Png),
    }
    .map_err(|err| ApiError::Internal(format!("cannot encode tile: {err}")))?;
    Ok(bytes)
}
