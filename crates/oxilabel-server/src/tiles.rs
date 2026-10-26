//! Tile pyramids for the annotation workspace. The first tile request for an image decodes it once
//! and writes every tile of every level to the cache; later requests read the cached files.
//! Viewing tiles are JPEG; analysis tiles (for the fuzzy fill and edge snapping, ADR 0002) are
//! the same pyramid as lossless PNG, generated the first time one is asked for.
//!
//! With `?filter=<key>` (see [`Filter::key`]) both are of the image through that enhancement
//! filter, each filter in its own cache folder; only the [`KEPT_FILTERS`] most recently used
//! filters of an image are kept.

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
    extract::{Path as UrlPath, Query, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Pixel, codecs::jpeg::JpegEncoder, imageops::FilterType};
use oxilabel_core::{
    filter::Filter,
    pyramid::{Pyramid, TILE_SIZE},
};
use serde::Deserialize;

use crate::{
    AppState,
    access_scope::AccessScope,
    enhance::{self, Pixels},
    error::ApiError,
    thumbnails::open_image,
    write_atomically,
};

const JPEG_QUALITY: u8 = 90;
/// Written last, so its presence means every tile of the pyramid is in the cache.
const COMPLETE_MARKER: &str = "complete";
/// How many filters (each filter and parameter set one) keep their pyramids per image.
pub const KEPT_FILTERS: usize = 12;
/// In a filter's folder; its modification time is when the filter was last asked for.
const USED_MARKER: &str = "used";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/images/{id}/tiles/{level}/{x}/{y}", get(|user, state, url, query| tile(user, state, url, query, Kind::View)))
        .route("/api/images/{id}/analysis/{level}/{x}/{y}", get(|user, state, url, query| tile(user, state, url, query, Kind::Analysis)))
}

#[derive(Deserialize)]
struct TileQuery {
    filter: Option<String>,
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
    scope: AccessScope,
    State(state): State<AppState>,
    UrlPath((id, level, x, y)): UrlPath<(i64, u32, u32, u32)>,
    Query(query): Query<TileQuery>,
    kind: Kind,
) -> Result<impl IntoResponse, ApiError> {
    let filter = match query.filter.as_deref() {
        None | Some("") => None,
        Some(key) => Some(Filter::parse(key).ok_or_else(|| ApiError::BadRequest(format!("unknown filter {key}")))?),
    };
    let visible = scope.image(&mut *state.pool.acquire().await?, id).await?;
    let pyramid = Pyramid::new(visible.image.width, visible.image.height);
    pyramid.tile_bounds(level, x, y).ok_or(ApiError::NotFound)?;

    let hash = visible.hash;
    // Named by content hash, so identical images share one pyramid; filters each get a folder
    // (named by their key) under `filters`.
    let image_dir = state.cache_dir.join("tiles").join(&hash);
    let filters_dir = image_dir.join("filters");
    let dir = match &filter {
        None => image_dir.join(kind.folder()),
        Some(filter) => {
            let filter_dir = filters_dir.join(filter.key());
            mark_used(&filter_dir)?;
            filter_dir.join(kind.folder())
        }
    };
    if !dir.join(COMPLETE_MARKER).exists() {
        let relative = dir.strip_prefix(&state.cache_dir).unwrap_or(&dir).to_string_lossy().into_owned();
        let lock = state.pyramid_locks.for_key(&relative);
        let _generating = lock.lock().await;
        if !dir.join(COMPLETE_MARKER).exists() {
            let source = visible.source;
            let dir = dir.clone();
            let filtered = filter.is_some();
            tokio::task::spawn_blocking(move || {
                generate(&source, &dir, pyramid, kind, filter.as_ref())?;
                if filtered {
                    forget_old_filters(&filters_dir);
                }
                Ok::<_, ApiError>(())
            })
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

/// Records that the filter of `filter_dir` is asked for now.
fn mark_used(filter_dir: &Path) -> Result<(), ApiError> {
    let failed = |err: std::io::Error| ApiError::Internal(format!("cannot mark a filter used: {err}"));
    fs::create_dir_all(filter_dir).map_err(failed)?;
    fs::File::options().create(true).append(true).open(filter_dir.join(USED_MARKER)).and_then(|file| file.set_modified(std::time::SystemTime::now())).map_err(failed)
}

/// Removes all but the [`KEPT_FILTERS`] most recently used filters in `filters_dir`. Every
/// request marks its filter used first, so filters being viewed or made are among the kept.
fn forget_old_filters(filters_dir: &Path) {
    let Ok(entries) = fs::read_dir(filters_dir) else { return };
    let used = |dir: &Path| fs::metadata(dir.join(USED_MARKER)).and_then(|m| m.modified()).ok();
    let mut filters: Vec<_> = entries.filter_map(|entry| entry.ok()).map(|entry| entry.path()).map(|dir| (used(&dir), dir)).collect();
    // Most recently used first.
    filters.sort_by_key(|filter| std::cmp::Reverse(filter.0));
    for (_, dir) in filters.into_iter().skip(KEPT_FILTERS) {
        fs::remove_dir_all(dir).ok();
    }
}

/// Decodes the source (through `filter`, if any) and writes all tiles of all levels, each level
/// downscaled from the previous one.
fn generate(source: &Path, dir: &Path, pyramid: Pyramid, kind: Kind, filter: Option<&Filter>) -> Result<(), ApiError> {
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
    let base = match filter {
        None => base,
        Some(filter) => filtered(&base, filter),
    };
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
    // Should the folder have been removed meanwhile (see `forget_old_filters`), it is not
    // marked complete, and the next request makes it again.
    if let Some(&(level, x, y)) = tiles.iter().find(|&&(level, x, y)| !tile_path(dir, level, x, y, kind).exists()) {
        return Err(ApiError::Internal(format!("tile {level}/{x}/{y} vanished while its pyramid was made")));
    }
    write_atomically(&dir.join(COMPLETE_MARKER), format!("{TILE_SIZE}\n").as_bytes())
        .map_err(|err| ApiError::Internal(format!("cannot cache tile: {err}")))
}

/// `image` (gray or RGB) through `filter`.
fn filtered(image: &DynamicImage, filter: &Filter) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    let channels = if matches!(image, DynamicImage::ImageLuma8(_)) { 1 } else { 3 };
    let pixels = Pixels { width: width as usize, height: height as usize, channels, data: image.as_bytes().to_vec() };
    let out = enhance::apply(filter, &pixels);
    const SAME_SIZE: &str = "filters keep the size";
    if out.channels == 1 {
        DynamicImage::ImageLuma8(ImageBuffer::from_raw(width, height, out.data).expect(SAME_SIZE))
    } else {
        DynamicImage::ImageRgb8(ImageBuffer::from_raw(width, height, out.data).expect(SAME_SIZE))
    }
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
