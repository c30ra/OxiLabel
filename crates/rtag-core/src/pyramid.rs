//! Layout of an image's tile pyramid. Level 0 is full resolution; each further level halves both
//! sides (rounding up) until the whole image fits in one tile. Every level is cut into square tiles
//! of `TILE_SIZE` pixels; tiles on the right and bottom edges may be smaller.

use crate::geometry::Rect;

pub const TILE_SIZE: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pyramid {
    pub width: u32,
    pub height: u32,
}

/// A tile's position and size in the pixels of its level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Pyramid {
    pub fn new(width: u32, height: u32) -> Self {
        Pyramid { width: width.max(1), height: height.max(1) }
    }

    /// Number of levels; the last one fits in a single tile.
    pub fn levels(&self) -> u32 {
        let mut level = 0;
        while {
            let (w, h) = self.level_size(level);
            w > TILE_SIZE || h > TILE_SIZE
        } {
            level += 1;
        }
        level + 1
    }

    /// Width and height of the image at `level`.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        let shrink = |side: u32| side.div_ceil(1 << level.min(31)).max(1);
        (shrink(self.width), shrink(self.height))
    }

    /// How many image pixels one pixel of `level` covers.
    pub fn level_scale(level: u32) -> f64 {
        f64::from(1u32 << level.min(31))
    }

    /// Tile columns and rows at `level`.
    pub fn tile_grid(&self, level: u32) -> (u32, u32) {
        let (w, h) = self.level_size(level);
        (w.div_ceil(TILE_SIZE), h.div_ceil(TILE_SIZE))
    }

    /// Where tile (`column`, `row`) of `level` lies, or `None` if there is no such tile.
    pub fn tile_bounds(&self, level: u32, column: u32, row: u32) -> Option<TileBounds> {
        let (columns, rows) = self.tile_grid(level);
        if level >= self.levels() || column >= columns || row >= rows {
            return None;
        }
        let (w, h) = self.level_size(level);
        let (x, y) = (column * TILE_SIZE, row * TILE_SIZE);
        Some(TileBounds { x, y, width: TILE_SIZE.min(w - x), height: TILE_SIZE.min(h - y) })
    }

    /// The coarsest level that still has at least one level pixel per screen pixel when the image is
    /// drawn at `scale` screen pixels per image pixel.
    pub fn level_for_scale(&self, scale: f64) -> u32 {
        if scale.is_nan() || scale <= 0.0 || scale >= 1.0 {
            return 0;
        }
        ((1.0 / scale).log2().floor() as u32).min(self.levels() - 1)
    }

    /// Tiles of `level` that overlap `region`, given in image coordinates.
    pub fn tiles_in(&self, level: u32, region: Rect) -> Vec<(u32, u32)> {
        let (columns, rows) = self.tile_grid(level);
        let span = Pyramid::level_scale(level) * f64::from(TILE_SIZE);
        let range = |min: f64, max: f64, count: u32| {
            let first = (min / span).floor().max(0.0) as u32;
            let last = ((max / span).ceil().max(0.0) as u32).min(count);
            first..last
        };
        let rows = range(region.min.y, region.max.y, rows);
        range(region.min.x, region.max.x, columns)
            .flat_map(|column| rows.clone().map(move |row| (column, row)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 6 MP image: 3000 × 2000.
    const SIX_MP: Pyramid = Pyramid { width: 3000, height: 2000 };

    #[test]
    fn levels_halve_until_the_image_fits_in_one_tile() {
        assert_eq!(SIX_MP.levels(), 4);
        assert_eq!(SIX_MP.level_size(0), (3000, 2000));
        assert_eq!(SIX_MP.level_size(1), (1500, 1000));
        assert_eq!(SIX_MP.level_size(3), (375, 250));
        assert_eq!(Pyramid::new(511, 7).levels(), 1);
        assert_eq!(Pyramid::new(513, 7).levels(), 2);
    }

    #[test]
    fn odd_sizes_round_up_so_no_pixel_is_lost() {
        let pyramid = Pyramid::new(1025, 3);
        assert_eq!(pyramid.level_size(1), (513, 2));
        assert_eq!(pyramid.level_size(2), (257, 1));
    }

    #[test]
    fn edge_tiles_are_cropped() {
        assert_eq!(SIX_MP.tile_grid(0), (6, 4));
        assert_eq!(SIX_MP.tile_bounds(0, 0, 0), Some(TileBounds { x: 0, y: 0, width: 512, height: 512 }));
        assert_eq!(SIX_MP.tile_bounds(0, 5, 3), Some(TileBounds { x: 2560, y: 1536, width: 440, height: 464 }));
        assert_eq!(SIX_MP.tile_bounds(3, 0, 0), Some(TileBounds { x: 0, y: 0, width: 375, height: 250 }));
    }

    #[test]
    fn tiles_outside_the_pyramid_do_not_exist() {
        assert_eq!(SIX_MP.tile_bounds(0, 6, 0), None);
        assert_eq!(SIX_MP.tile_bounds(0, 0, 4), None);
        assert_eq!(SIX_MP.tile_bounds(4, 0, 0), None);
    }

    #[test]
    fn the_level_matches_the_zoom() {
        assert_eq!(SIX_MP.level_for_scale(4.0), 0);
        assert_eq!(SIX_MP.level_for_scale(1.0), 0);
        assert_eq!(SIX_MP.level_for_scale(0.6), 0);
        assert_eq!(SIX_MP.level_for_scale(0.5), 1);
        assert_eq!(SIX_MP.level_for_scale(0.3), 1);
        assert_eq!(SIX_MP.level_for_scale(0.25), 2);
        assert_eq!(SIX_MP.level_for_scale(0.01), 3);
    }

    #[test]
    fn only_tiles_in_view_are_listed() {
        // Image pixels 600..1100 × 0..100 at level 0 touch columns 1 and 2 of row 0.
        assert_eq!(SIX_MP.tiles_in(0, Rect::new(600.0, 0.0, 1100.0, 100.0)), vec![(1, 0), (2, 0)]);
        // At level 1 a tile covers 1024 image pixels.
        assert_eq!(SIX_MP.tiles_in(1, Rect::new(600.0, 0.0, 1100.0, 100.0)), vec![(0, 0), (1, 0)]);
        // A region larger than the image is clamped; one entirely outside gives nothing.
        assert_eq!(SIX_MP.tiles_in(3, Rect::new(-500.0, -500.0, 9000.0, 9000.0)), vec![(0, 0)]);
        assert!(SIX_MP.tiles_in(0, Rect::new(-500.0, -500.0, -1.0, -1.0)).is_empty());
    }
}
