//! Fuzzy fill (magic wand): the region of pixels similar to a clicked one, found on the tiles of
//! one pyramid level as the browser loads them (ADR 0002). Growing stops at tiles not loaded yet
//! and names them, so the caller can fetch just those and grow again.

use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::{
    mask::{Bitmap, PixelRect},
    pyramid::{Pyramid, TILE_SIZE},
};

/// The pixels of one tile: `channels` bytes per pixel (gray, RGB or RGBA), row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub pixels: Vec<u8>,
}

impl Raster {
    /// The colour at (`x`, `y`) of the tile; alpha is ignored.
    pub(crate) fn colour(&self, x: u32, y: u32) -> [u8; 3] {
        let channels = usize::from(self.channels);
        let at = (y as usize * self.width as usize + x as usize) * channels;
        match channels {
            1 | 2 => [self.pixels[at]; 3],
            _ => [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]],
        }
    }
}

/// The loaded tiles of one level, by (column, row).
pub type Tiles = HashMap<(u32, u32), Raster>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillSettings {
    /// The largest difference in any colour channel from the seed that still counts as similar.
    pub threshold: u8,
    /// `None`: only pixels connected to the seed. Otherwise every similar pixel inside this
    /// area (in pixels of the level), e.g. the visible part of the image.
    pub within: Option<PixelRect>,
    /// Growing stops once the region has this many pixels (of the level).
    pub max_pixels: u64,
}

/// What a fill found so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Growth {
    /// The region in pixels of the level.
    pub region: Bitmap,
    /// Tiles the region reached that are not loaded: fetch them and grow again.
    pub missing: Vec<(u32, u32)>,
    /// Whether growing stopped at `max_pixels`.
    pub capped: bool,
}

/// The region of `level` similar to the pixel `seed` (in pixels of that level), grown over the
/// loaded `tiles`.
pub fn grow(pyramid: Pyramid, level: u32, tiles: &Tiles, seed: (u32, u32), settings: FillSettings) -> Growth {
    let mut growth = Growth { region: Bitmap::default(), missing: Vec::new(), capped: false };
    let tile_of = |x: u32, y: u32| (x / TILE_SIZE, y / TILE_SIZE);
    // A grid of the level's tiles: one array lookup per pixel instead of a hash.
    let (columns, rows) = pyramid.tile_grid(level);
    let mut grid: Vec<Option<&Raster>> = vec![None; columns as usize * rows as usize];
    for (&(column, row), tile) in tiles {
        if column < columns && row < rows {
            grid[(row * columns + column) as usize] = Some(tile);
        }
    }
    let colour_at = |x: u32, y: u32| {
        let (column, row) = tile_of(x, y);
        grid[(row * columns + column) as usize].map(|tile| tile.colour(x - column * TILE_SIZE, y - row * TILE_SIZE))
    };
    let Some(seed_colour) = colour_at(seed.0, seed.1) else {
        growth.missing.push(tile_of(seed.0, seed.1));
        return growth;
    };
    let similar = |colour: [u8; 3]| colour.iter().zip(seed_colour).all(|(a, b)| a.abs_diff(b) <= settings.threshold);

    // The region can only lie on loaded tiles.
    let (width, height) = pyramid.level_size(level);
    let area = tiles
        .keys()
        .filter_map(|&(column, row)| pyramid.tile_bounds(level, column, row))
        .map(|b| PixelRect { left: b.x, top: b.y, width: b.width, height: b.height })
        .fold(PixelRect::default(), |all, tile| all.union(&tile));
    let mut region = Bitmap { area, pixels: vec![0; area.width as usize * area.height as usize] };
    let index = |x: u32, y: u32| (y - area.top) as usize * area.width as usize + (x - area.left) as usize;
    let mut missing = BTreeSet::new();
    let mut size = 1u64;

    if let Some(within) = settings.within {
        let within = within.intersection(&PixelRect { left: 0, top: 0, width, height });
        size = 0;
        'tiles: for row in within.top / TILE_SIZE..within.bottom().div_ceil(TILE_SIZE) {
            for column in within.left / TILE_SIZE..within.right().div_ceil(TILE_SIZE) {
                let Some(tile) = tiles.get(&(column, row)) else {
                    missing.insert((column, row));
                    continue;
                };
                let (left, top) = (column * TILE_SIZE, row * TILE_SIZE);
                let part = within.intersection(&PixelRect { left, top, width: tile.width, height: tile.height });
                for y in part.top..part.bottom() {
                    for x in part.left..part.right() {
                        if similar(tile.colour(x - left, y - top)) {
                            region.pixels[index(x, y)] = 1;
                            size += 1;
                            if size >= settings.max_pixels {
                                growth.capped = true;
                                break 'tiles;
                            }
                        }
                    }
                }
            }
        }
        growth.region = region;
        growth.missing = missing.into_iter().collect();
        return growth;
    }

    // Breadth first, so a region stopped at the maximum size is a compact patch around the seed.
    let mut pending = VecDeque::from([seed]);
    region.pixels[index(seed.0, seed.1)] = 1;
    'growing: while let Some((x, y)) = pending.pop_front() {
        let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
        for (nx, ny) in neighbours {
            if nx >= width || ny >= height {
                continue;
            }
            match colour_at(nx, ny) {
                None => {
                    missing.insert(tile_of(nx, ny));
                }
                Some(colour) if similar(colour) && region.pixels[index(nx, ny)] == 0 => {
                    if size >= settings.max_pixels {
                        growth.capped = true;
                        break 'growing;
                    }
                    region.pixels[index(nx, ny)] = 1;
                    size += 1;
                    pending.push_back((nx, ny));
                }
                Some(_) => {}
            }
        }
    }
    growth.region = region;
    growth.missing = missing.into_iter().collect();
    growth
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-channel tile of `size` filled with `background`, with `boxes` (left, top, right,
    /// bottom, value) drawn on it.
    fn tile(size: (u32, u32), background: u8, boxes: &[(u32, u32, u32, u32, u8)]) -> Raster {
        let mut pixels = vec![background; (size.0 * size.1) as usize];
        for &(left, top, right, bottom, value) in boxes {
            for y in top..bottom {
                for x in left..right {
                    pixels[(y * size.0 + x) as usize] = value;
                }
            }
        }
        Raster { width: size.0, height: size.1, channels: 1, pixels }
    }

    const CONTIGUOUS: FillSettings = FillSettings { threshold: 10, within: None, max_pixels: u64::MAX };

    #[test]
    fn the_region_is_the_connected_similar_pixels_around_the_seed() {
        let pyramid = Pyramid::new(512, 512);
        // A dark square with a slightly lighter corner, and a separate dark square.
        let tiles = Tiles::from([((0, 0), tile((512, 512), 200, &[(10, 10, 20, 20, 50), (10, 10, 12, 12, 58), (40, 40, 45, 45, 50)]))]);
        let growth = grow(pyramid, 0, &tiles, (15, 15), CONTIGUOUS);
        assert!(growth.missing.is_empty());
        assert_eq!(growth.region.bounds(), Some(PixelRect { left: 10, top: 10, width: 10, height: 10 }));
        assert!(growth.region.get(10, 10), "within the threshold");
        assert!(!growth.region.get(40, 40), "similar but not connected");
    }

    #[test]
    fn growing_names_the_unloaded_tiles_it_reaches_and_continues_once_they_are_loaded() {
        // 1100 × 600: three columns and two rows of tiles. A dark band crosses from tile (0, 0)
        // into (1, 0); nothing dark reaches the other tiles.
        let pyramid = Pyramid::new(1100, 600);
        let mut tiles = Tiles::from([((0, 0), tile((512, 512), 200, &[(400, 100, 512, 120, 50)]))]);
        let first = grow(pyramid, 0, &tiles, (450, 110), CONTIGUOUS);
        assert_eq!(first.missing, vec![(1, 0)]);
        assert_eq!(first.region.bounds(), Some(PixelRect { left: 400, top: 100, width: 112, height: 20 }));

        tiles.insert((1, 0), tile((512, 512), 200, &[(0, 100, 88, 120, 50)]));
        let second = grow(pyramid, 0, &tiles, (450, 110), CONTIGUOUS);
        assert!(second.missing.is_empty());
        assert_eq!(second.region.bounds(), Some(PixelRect { left: 400, top: 100, width: 200, height: 20 }));
    }

    #[test]
    fn a_seed_on_an_unloaded_tile_asks_for_that_tile() {
        let growth = grow(Pyramid::new(1100, 600), 0, &Tiles::new(), (600, 10), CONTIGUOUS);
        assert_eq!((growth.missing, growth.region.bounds()), (vec![(1, 0)], None));
    }

    #[test]
    fn growing_stops_at_the_maximum_size() {
        let tiles = Tiles::from([((0, 0), tile((512, 512), 200, &[]))]);
        let growth = grow(Pyramid::new(512, 512), 0, &tiles, (5, 5), FillSettings { max_pixels: 1000, ..CONTIGUOUS });
        assert!(growth.capped);
        let set = growth.region.pixels.iter().filter(|&&p| p != 0).count();
        assert_eq!(set, 1000);
        let bounds = growth.region.bounds().unwrap();
        assert!(bounds.width < 60 && bounds.height < 60, "a compact patch around the seed, not a long strip: {bounds:?}");
    }

    #[test]
    fn global_mode_takes_every_similar_pixel_within_its_area() {
        let pyramid = Pyramid::new(1100, 600);
        let tiles = Tiles::from([
            ((0, 0), tile((512, 512), 200, &[(10, 10, 20, 20, 50)])),
            ((1, 0), tile((512, 512), 200, &[(100, 100, 102, 101, 55), (300, 300, 310, 310, 50)])),
        ]);
        // The area ends at x 700, before the second tile's lower box; it reaches into row 1.
        let within = Some(PixelRect { left: 0, top: 0, width: 700, height: 520 });
        let growth = grow(pyramid, 0, &tiles, (15, 15), FillSettings { within, ..CONTIGUOUS });
        assert_eq!(growth.missing, vec![(0, 1), (1, 1)], "unloaded tiles of the area");
        assert_eq!(growth.region.bounds(), Some(PixelRect { left: 10, top: 10, width: 604, height: 91 }));
        assert!(growth.region.get(613, 100) && !growth.region.get(30, 30));
    }
}
