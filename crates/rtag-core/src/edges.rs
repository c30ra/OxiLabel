//! Edge snapping (live-wire, like GIMP's intelligent scissors): an edge-strength map of a window
//! of one pyramid level, the strongest edge near a point, and the cheapest path between two points
//! that runs along strong edges. Coordinates are pixels of the level.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::{
    fill::Tiles,
    mask::PixelRect,
    pyramid::{Pyramid, TILE_SIZE},
};

/// Edges weaker than this share of the window's strongest one, or than this Sobel strength (on
/// 0–255 values), are texture, not edges to snap to.
const FAINT_SHARE: f32 = 0.2;
const FAINT_STRENGTH: f32 = 8.0;

/// Gradient strength (Sobel) over a window of one level.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeMap {
    pub area: PixelRect,
    strength: Vec<f32>,
    strongest: f32,
}

impl EdgeMap {
    /// The map of `window` (clipped to the level), or the tiles it needs that are not loaded.
    pub fn new(
        pyramid: Pyramid,
        level: u32,
        tiles: &Tiles,
        window: PixelRect,
    ) -> Result<EdgeMap, Vec<(u32, u32)>> {
        let (width, height) = pyramid.level_size(level);
        let area = window.intersection(&PixelRect {
            left: 0,
            top: 0,
            width,
            height,
        });
        let mut missing = Vec::new();
        if !area.is_empty() {
            for row in area.top / TILE_SIZE..area.bottom().div_ceil(TILE_SIZE) {
                for column in area.left / TILE_SIZE..area.right().div_ceil(TILE_SIZE) {
                    if !tiles.contains_key(&(column, row)) {
                        missing.push((column, row));
                    }
                }
            }
        }
        // An empty window (outside the level) has no map and needs no tiles.
        if area.is_empty() || !missing.is_empty() {
            missing.sort_unstable();
            return Err(missing);
        }
        let gray: Vec<f32> = (area.top..area.bottom())
            .flat_map(|y| (area.left..area.right()).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (column, row) = (x / TILE_SIZE, y / TILE_SIZE);
                let [r, g, b] =
                    tiles[&(column, row)].colour(x - column * TILE_SIZE, y - row * TILE_SIZE);
                (f32::from(r) + f32::from(g) + f32::from(b)) / 3.0
            })
            .collect();
        let (w, h) = (area.width as usize, area.height as usize);
        // Neighbours past the window's edge repeat the edge pixel.
        let at = |x: isize, y: isize| {
            gray[y.clamp(0, h as isize - 1) as usize * w + x.clamp(0, w as isize - 1) as usize]
        };
        let mut strength = Vec::with_capacity(w * h);
        for y in 0..h as isize {
            for x in 0..w as isize {
                let gx = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)
                    - at(x - 1, y - 1)
                    - 2.0 * at(x - 1, y)
                    - at(x - 1, y + 1);
                let gy = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)
                    - at(x - 1, y - 1)
                    - 2.0 * at(x, y - 1)
                    - at(x + 1, y - 1);
                strength.push(gx.hypot(gy));
            }
        }
        let strongest = strength.iter().copied().fold(0.0, f32::max);
        Ok(EdgeMap {
            area,
            strength,
            strongest,
        })
    }

    fn index(&self, (x, y): (u32, u32)) -> usize {
        (y - self.area.top) as usize * self.area.width as usize + (x - self.area.left) as usize
    }

    /// `point` moved into the map.
    fn clamp(&self, (x, y): (u32, u32)) -> (u32, u32) {
        (
            x.clamp(self.area.left, self.area.right() - 1),
            y.clamp(self.area.top, self.area.bottom() - 1),
        )
    }

    /// The pixel with the strongest edge within `radius` of `point` (the nearest of equals), or
    /// `point` itself if nothing there is an edge.
    pub fn strongest_near(&self, point: (u32, u32), radius: u32) -> (u32, u32) {
        let point = self.clamp(point);
        let (mut best, mut best_strength, mut best_distance) = (point, 0.0f32, 0u64);
        let reach = |v: u32, low: u32, high: u32| {
            (
                v.saturating_sub(radius).max(low),
                v.saturating_add(radius).min(high - 1),
            )
        };
        let (left, right) = reach(point.0, self.area.left, self.area.right());
        let (top, bottom) = reach(point.1, self.area.top, self.area.bottom());
        for y in top..=bottom {
            for x in left..=right {
                let distance = u64::from(x.abs_diff(point.0)).pow(2) + u64::from(y.abs_diff(point.1)).pow(2);
                if distance > u64::from(radius).pow(2) {
                    continue;
                }
                let strength = self.strength[self.index((x, y))];
                if strength > best_strength || (strength == best_strength && strength > 0.0 && distance < best_distance) {
                    (best, best_strength, best_distance) = ((x, y), strength, distance);
                }
            }
        }
        // Faint texture is not an edge worth snapping to.
        if best_strength < self.strongest * FAINT_SHARE || best_strength < FAINT_STRENGTH { point } else { best }
    }

    /// The cheapest path of 8-connected pixels from `from` to `to` (both moved into the map),
    /// where strong edges are cheap to walk along.
    pub fn live_wire(&self, from: (u32, u32), to: (u32, u32)) -> Vec<(u32, u32)> {
        let (from, to) = (self.clamp(from), self.clamp(to));
        let (w, h) = (self.area.width as i64, self.area.height as i64);
        let strongest = self.strongest.max(1.0);
        // Costs in thousandths; a small base cost keeps paths from wandering along long edges.
        let step_cost = |at: usize, diagonal: bool| {
            let weak = 1.0 - self.strength[at] / strongest;
            let cost = (weak * weak + 0.05)
                * if diagonal {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
            (cost * 1000.0) as u64
        };
        let mut cost = vec![u64::MAX; self.strength.len()];
        let mut previous = vec![usize::MAX; self.strength.len()];
        let (start, goal) = (self.index(from), self.index(to));
        cost[start] = 0;
        let mut queue = BinaryHeap::from([Reverse((0u64, start))]);
        while let Some(Reverse((spent, at))) = queue.pop() {
            if at == goal {
                break;
            }
            if spent > cost[at] {
                continue;
            }
            let (x, y) = ((at as i64) % w, (at as i64) / w);
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let next = (ny * w + nx) as usize;
                let total = spent + step_cost(next, dx != 0 && dy != 0);
                if total < cost[next] {
                    cost[next] = total;
                    previous[next] = at;
                    queue.push(Reverse((total, next)));
                }
            }
        }
        let mut path = vec![goal];
        while let Some(&last) = path.last() {
            if last == start || previous[last] == usize::MAX {
                break;
            }
            path.push(previous[last]);
        }
        path.reverse();
        let width = self.area.width as usize;
        path.into_iter()
            .map(|at| {
                (
                    self.area.left + (at % width) as u32,
                    self.area.top + (at / width) as u32,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::Raster;

    /// A one-tile gray image of `width` × `height` whose pixel (x, y) is `value(x, y)`.
    fn image(width: u32, height: u32, value: impl Fn(u32, u32) -> u8) -> (Pyramid, Tiles) {
        let pixels = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| value(x, y))
            .collect();
        (
            Pyramid::new(width, height),
            Tiles::from([(
                (0, 0),
                Raster {
                    width,
                    height,
                    channels: 1,
                    pixels,
                },
            )]),
        )
    }

    fn whole(pyramid: Pyramid) -> PixelRect {
        PixelRect {
            left: 0,
            top: 0,
            width: pyramid.width,
            height: pyramid.height,
        }
    }

    #[test]
    fn the_strongest_edge_near_a_point_is_found_within_the_radius() {
        // Dark left of x = 50, light right of it.
        let (pyramid, tiles) = image(100, 60, |x, _| if x < 50 { 30 } else { 220 });
        let map = EdgeMap::new(pyramid, 0, &tiles, whole(pyramid)).unwrap();
        let (x, y) = map.strongest_near((56, 30), 8);
        assert!(
            (49..=50).contains(&x) && y.abs_diff(30) <= 8,
            "found ({x}, {y})"
        );
        assert_eq!(
            map.strongest_near((80, 30), 8),
            (80, 30),
            "no edge in reach: the point itself"
        );
    }

    #[test]
    fn the_live_wire_follows_a_curved_edge_between_two_points_on_it() {
        let boundary = |y: u32| 50.0 + 15.0 * (f64::from(y) / 12.0).sin();
        let (pyramid, tiles) = image(
            100,
            100,
            |x, y| if f64::from(x) < boundary(y) { 30 } else { 220 },
        );
        let map = EdgeMap::new(pyramid, 0, &tiles, whole(pyramid)).unwrap();
        let from = (boundary(5).round() as u32, 5);
        let to = (boundary(95).round() as u32, 95);
        let path = map.live_wire(from, to);
        assert_eq!((path[0], *path.last().unwrap()), (from, to));
        let worst = path
            .iter()
            .map(|&(x, y)| (f64::from(x) - boundary(y)).abs())
            .fold(0.0, f64::max);
        assert!(
            worst <= 2.0,
            "the path strays {worst:.1} px from the edge (a straight line strays 15)"
        );
    }

    #[test]
    fn a_map_needs_every_tile_of_its_window() {
        let pyramid = Pyramid::new(1100, 600);
        let tiles = Tiles::from([(
            (0, 0),
            Raster {
                width: 512,
                height: 512,
                channels: 1,
                pixels: vec![0; 512 * 512],
            },
        )]);
        let window = PixelRect {
            left: 500,
            top: 500,
            width: 20,
            height: 20,
        };
        assert_eq!(
            EdgeMap::new(pyramid, 0, &tiles, window).unwrap_err(),
            vec![(0, 1), (1, 0), (1, 1)]
        );
    }
}
