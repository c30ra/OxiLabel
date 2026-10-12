//! Masks: painted regions in original-image pixels. A `Mask` is the stored form (the rows of its
//! bounding box, run-length encoded); a `Bitmap` is the decoded form that brushes paint into.
//! Pixel (x, y) covers the square from (x, y) to (x + 1, y + 1) in image coordinates.

use serde::{Deserialize, Serialize};

use crate::geometry::{Point, distance_to_segment};

/// Run lengths of `pixels` (0 unset, anything else set), alternating unset and set runs and
/// starting with an unset run, which is 0 when the first pixel is set.
pub fn encode(pixels: &[u8]) -> Vec<u32> {
    let mut runs = Vec::new();
    let (mut set, mut length) = (false, 0u32);
    for &pixel in pixels {
        if (pixel != 0) != set {
            runs.push(length);
            (set, length) = (!set, 0);
        }
        length += 1;
    }
    if length > 0 {
        runs.push(length);
    }
    runs
}

/// A rectangle of whole image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn right(&self) -> u32 {
        self.left + self.width
    }

    pub fn bottom(&self) -> u32 {
        self.top + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    fn from_edges(left: u32, top: u32, right: u32, bottom: u32) -> PixelRect {
        PixelRect { left, top, width: right.saturating_sub(left), height: bottom.saturating_sub(top) }
    }

    /// The smallest rectangle containing both; an empty one adds nothing.
    pub fn union(&self, other: &PixelRect) -> PixelRect {
        match (self.is_empty(), other.is_empty()) {
            (true, _) => *other,
            (_, true) => *self,
            _ => PixelRect::from_edges(
                self.left.min(other.left),
                self.top.min(other.top),
                self.right().max(other.right()),
                self.bottom().max(other.bottom()),
            ),
        }
    }

    pub fn intersection(&self, other: &PixelRect) -> PixelRect {
        PixelRect::from_edges(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right().min(other.right()),
            self.bottom().min(other.bottom()),
        )
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.left && x < self.right() && y >= self.top && y < self.bottom()
    }
}

/// A decoded mask: one byte per pixel (0 unset, 1 set) over `area`, in image pixels. It may have
/// unset margins; `to_mask` crops them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bitmap {
    pub area: PixelRect,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    pub fn get(&self, x: u32, y: u32) -> bool {
        self.area.contains(x, y) && self.pixels[self.offset(x, y)] != 0
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        (y - self.area.top) as usize * self.area.width as usize + (x - self.area.left) as usize
    }

    /// Sets the pixels whose centres are within `radius` of the segment `from`–`to` (a round
    /// brush dragged along it), inside an `image` of that size. Returns the pixels it covered.
    pub fn paint(&mut self, from: Point, to: Point, radius: f64, image: (u32, u32)) -> PixelRect {
        let covered = brush_area(from, to, radius, image);
        if covered.is_empty() {
            return covered;
        }
        self.grow_to(covered, image);
        self.stroke(from, to, radius, covered, 1);
        covered
    }

    /// Clears the pixels a round eraser dragged from `from` to `to` covers. Returns the part of
    /// the bitmap it touched.
    pub fn erase(&mut self, from: Point, to: Point, radius: f64) -> PixelRect {
        let covered = brush_area(from, to, radius, (u32::MAX, u32::MAX)).intersection(&self.area);
        if !covered.is_empty() {
            self.stroke(from, to, radius, covered, 0);
        }
        covered
    }

    /// Whether a pixel of `region` is set in both.
    pub fn overlaps(&self, other: &Bitmap, region: PixelRect) -> bool {
        let common = self.area.intersection(&other.area).intersection(&region);
        (common.top..common.bottom())
            .any(|y| (common.left..common.right()).any(|x| self.pixels[self.offset(x, y)] != 0 && other.get(x, y)))
    }

    /// Adds the set pixels of `other` (inside an `image` of that size).
    pub fn merge(&mut self, other: &Bitmap, image: (u32, u32)) {
        let Some(bounds) = other.bounds() else { return };
        self.grow_to(bounds, image);
        for y in bounds.top..bounds.bottom() {
            for x in bounds.left..bounds.right() {
                if other.get(x, y) {
                    let offset = self.offset(x, y);
                    self.pixels[offset] = 1;
                }
            }
        }
    }

    /// Clears the pixels set in `other`. Returns the part of the bitmap it looked at.
    pub fn subtract(&mut self, other: &Bitmap) -> PixelRect {
        let common = self.area.intersection(&other.area);
        for y in common.top..common.bottom() {
            for x in common.left..common.right() {
                if other.get(x, y) {
                    let offset = self.offset(x, y);
                    self.pixels[offset] = 0;
                }
            }
        }
        common
    }

    /// A copy without set patches smaller than `min_area` pixels, and with enclosed holes smaller
    /// than that filled. Pixels touch along edges, not corners.
    pub fn cleaned(self, min_area: usize) -> Bitmap {
        if min_area <= 1 {
            return self;
        }
        let mut cleaned = self.clone();
        let (width, height) = (self.area.width as usize, self.area.height as usize);
        let mut seen = vec![false; self.pixels.len()];
        for start in 0..self.pixels.len() {
            if seen[start] {
                continue;
            }
            // Flood the patch of equal pixels around `start`.
            let value = self.pixels[start];
            let (mut patch, mut pending, mut on_border) = (Vec::new(), vec![start], false);
            seen[start] = true;
            while let Some(at) = pending.pop() {
                patch.push(at);
                let (x, y) = (at % width, at / width);
                on_border |= x == 0 || y == 0 || x + 1 == width || y + 1 == height;
                let neighbours = [
                    (x > 0).then(|| at - 1),
                    (x + 1 < width).then(|| at + 1),
                    (y > 0).then(|| at - width),
                    (y + 1 < height).then(|| at + width),
                ];
                for next in neighbours.into_iter().flatten() {
                    if !seen[next] && self.pixels[next] == value {
                        seen[next] = true;
                        pending.push(next);
                    }
                }
            }
            // A hole is an unset patch the set pixels enclose, so not one at the edge.
            let small = patch.len() < min_area && (value != 0 || !on_border);
            if small {
                patch.into_iter().for_each(|at| cleaned.pixels[at] = u8::from(value == 0));
            }
        }
        cleaned
    }

    /// The bitmap of a pyramid level whose pixels cover `factor` × `factor` image pixels, in the
    /// pixels of an `image` of that size.
    pub fn scaled_up(&self, factor: u32, image: (u32, u32)) -> Bitmap {
        let Some(bounds) = self.bounds() else { return Bitmap::default() };
        let area = PixelRect::from_edges(
            bounds.left * factor,
            bounds.top * factor,
            (bounds.right() * factor).min(image.0),
            (bounds.bottom() * factor).min(image.1),
        );
        let mut scaled = Bitmap { area, pixels: vec![0; area.width as usize * area.height as usize] };
        for y in area.top..area.bottom() {
            for x in area.left..area.right() {
                if self.get(x / factor, y / factor) {
                    let offset = scaled.offset(x, y);
                    scaled.pixels[offset] = 1;
                }
            }
        }
        scaled
    }

    /// Makes room for `needed`, with some margin so a stroke does not reallocate on every move.
    fn grow_to(&mut self, needed: PixelRect, image: (u32, u32)) {
        if self.area.union(&needed) == self.area {
            return;
        }
        let margin = 64.max(self.area.width.max(self.area.height) / 2);
        let wanted = self.area.union(&needed);
        let area = PixelRect::from_edges(
            wanted.left.saturating_sub(margin),
            wanted.top.saturating_sub(margin),
            (wanted.right() + margin).min(image.0),
            (wanted.bottom() + margin).min(image.1),
        );
        let mut pixels = vec![0; area.width as usize * area.height as usize];
        for y in self.area.top..self.area.bottom() {
            let from = self.offset(self.area.left, y);
            let to = (y - area.top) as usize * area.width as usize + (self.area.left - area.left) as usize;
            pixels[to..to + self.area.width as usize].copy_from_slice(&self.pixels[from..from + self.area.width as usize]);
        }
        *self = Bitmap { area, pixels };
    }

    /// Writes `value` to the pixels of `within` that the brush covers.
    fn stroke(&mut self, from: Point, to: Point, radius: f64, within: PixelRect, value: u8) {
        for y in within.top..within.bottom() {
            for x in within.left..within.right() {
                let centre = Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if distance_to_segment(centre, from, to) <= radius {
                    let offset = self.offset(x, y);
                    self.pixels[offset] = value;
                }
            }
        }
    }

    /// The tight bounding box of the set pixels, if any.
    pub fn bounds(&self) -> Option<PixelRect> {
        let width = self.area.width as usize;
        let rows: Vec<usize> = (0..self.area.height as usize)
            .filter(|&row| self.pixels[row * width..(row + 1) * width].iter().any(|&p| p != 0))
            .collect();
        let (first, last) = (*rows.first()?, *rows.last()?);
        let (mut left, mut right) = (width, 0);
        for row in first..=last {
            let line = &self.pixels[row * width..(row + 1) * width];
            if let Some(start) = line.iter().position(|&p| p != 0) {
                left = left.min(start);
                right = right.max(line.iter().rposition(|&p| p != 0).unwrap() + 1);
            }
        }
        Some(PixelRect {
            left: self.area.left + left as u32,
            top: self.area.top + first as u32,
            width: (right - left) as u32,
            height: (last - first + 1) as u32,
        })
    }

    /// A copy without unset margins (empty if nothing is set).
    pub fn cropped(&self) -> Bitmap {
        let Some(bounds) = self.bounds() else { return Bitmap::default() };
        let mut pixels = Vec::with_capacity(bounds.width as usize * bounds.height as usize);
        for y in bounds.top..bounds.bottom() {
            let start = self.offset(bounds.left, y);
            pixels.extend_from_slice(&self.pixels[start..start + bounds.width as usize]);
        }
        Bitmap { area: bounds, pixels }
    }

    /// The stored form, cropped to the set pixels; `None` if nothing is set.
    pub fn to_mask(&self) -> Option<Mask> {
        let Bitmap { area, pixels } = self.cropped();
        (!area.is_empty()).then(|| Mask { left: area.left, top: area.top, width: area.width, height: area.height, rle: encode(&pixels) })
    }
}

/// The pixels of an `image` whose centres a round brush of `radius` dragged from `from` to `to`
/// may cover.
fn brush_area(from: Point, to: Point, radius: f64, image: (u32, u32)) -> PixelRect {
    let first = |v: f64| (v - radius - 0.5).ceil().max(0.0);
    let last = |v: f64, size: u32| (v + radius - 0.5).floor().min(f64::from(size) - 1.0);
    let (left, top) = (first(from.x.min(to.x)), first(from.y.min(to.y)));
    let (right, bottom) = (last(from.x.max(to.x), image.0), last(from.y.max(to.y), image.1));
    if !(left <= right && top <= bottom) {
        return PixelRect::default();
    }
    PixelRect::from_edges(left as u32, top as u32, right as u32 + 1, bottom as u32 + 1)
}

/// A mask as stored: the rows of its bounding box in image pixels, run-length encoded (see
/// `encode`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub rle: Vec<u32>,
}

impl Mask {
    pub fn area(&self) -> PixelRect {
        PixelRect { left: self.left, top: self.top, width: self.width, height: self.height }
    }

    pub fn to_bitmap(&self) -> Bitmap {
        let mut pixels = decode(&self.rle);
        pixels.resize(self.width as usize * self.height as usize, 0);
        Bitmap { area: self.area(), pixels }
    }

    /// Checks that the runs fill the box, something is set, and it lies inside a `width` ×
    /// `height` image.
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        let area = self.area();
        if area.is_empty() || self.rle.iter().map(|&run| u64::from(run)).sum::<u64>() != u64::from(area.width) * u64::from(area.height) {
            return Err("a mask's runs must cover its box exactly".into());
        }
        if !self.rle.iter().skip(1).step_by(2).any(|&run| run > 0) {
            return Err("a mask needs at least one painted pixel".into());
        }
        if u64::from(self.left) + u64::from(self.width) > u64::from(width) || u64::from(self.top) + u64::from(self.height) > u64::from(height) {
            return Err(format!("the shape must lie inside the {width} × {height} image"));
        }
        Ok(())
    }

    /// Whether the pixel under `point` is set (read straight from the runs).
    pub fn contains(&self, point: Point) -> bool {
        if point.x < 0.0 || point.y < 0.0 || !self.area().contains(point.x as u32, point.y as u32) {
            return false;
        }
        let index = (point.y as u32 - self.top) as u64 * u64::from(self.width) + u64::from(point.x as u32 - self.left);
        let mut end = 0u64;
        for (i, &run) in self.rle.iter().enumerate() {
            end += u64::from(run);
            if index < end {
                return i % 2 == 1;
            }
        }
        false
    }
}

/// The pixels (0 or 1) of the runs made by `encode`.
pub fn decode(runs: &[u32]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(runs.iter().map(|&run| run as usize).sum());
    for (i, &run) in runs.iter().enumerate() {
        pixels.extend(std::iter::repeat_n((i % 2) as u8, run as usize));
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_counts_alternating_runs_starting_with_unset_pixels() {
        assert_eq!(encode(&[0, 0, 1, 1, 1, 0]), vec![2, 3, 1]);
        assert_eq!(encode(&[1, 1, 0]), vec![0, 2, 1]);
        assert_eq!(encode(&[]), Vec::<u32>::new());
        assert_eq!(decode(&[2, 3, 1]), vec![0, 0, 1, 1, 1, 0]);
        assert_eq!(decode(&[0, 2, 1]), vec![1, 1, 0]);
    }

    const IMAGE: (u32, u32) = (100, 80);

    fn dab(at: (f64, f64), radius: f64) -> Bitmap {
        let mut bitmap = Bitmap::default();
        let at = Point::new(at.0, at.1);
        bitmap.paint(at, at, radius, IMAGE);
        bitmap
    }

    #[test]
    fn a_brush_dab_sets_the_pixels_whose_centres_it_covers() {
        let mask = dab((10.5, 10.5), 1.0).to_mask().unwrap();
        assert_eq!(mask, Mask { left: 9, top: 9, width: 3, height: 3, rle: vec![1, 1, 1, 3, 1, 1, 1] });
    }

    #[test]
    fn painting_stops_at_the_image_border() {
        let mask = dab((0.5, 0.5), 1.0).to_mask().unwrap();
        assert_eq!(mask, Mask { left: 0, top: 0, width: 2, height: 2, rle: vec![0, 3, 1] });
        assert_eq!(dab((-20.0, 5.0), 3.0).to_mask(), None, "entirely outside");
    }

    #[test]
    fn a_dragged_brush_covers_the_whole_segment() {
        let mut bitmap = Bitmap::default();
        bitmap.paint(Point::new(10.5, 10.5), Point::new(20.5, 10.5), 0.5, IMAGE);
        assert_eq!(bitmap.to_mask().unwrap(), Mask { left: 10, top: 10, width: 11, height: 1, rle: vec![0, 11] });
    }

    #[test]
    fn stored_masks_decode_to_the_same_pixels_and_hit_test_by_pixel() {
        let mask = dab((10.5, 10.5), 1.0).to_mask().unwrap();
        let bitmap = mask.to_bitmap();
        assert_eq!(bitmap.to_mask().unwrap(), mask);
        assert!(mask.contains(Point::new(10.2, 9.9)), "top arm of the plus");
        assert!(!mask.contains(Point::new(9.5, 9.5)), "unset corner of its box");
        assert!(!mask.contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn erasing_clears_pixels_and_can_leave_nothing() {
        let mut bitmap = dab((10.5, 10.5), 3.0);
        let centre = Point::new(10.5, 10.5);
        bitmap.erase(centre, centre, 1.0);
        assert!(!bitmap.get(10, 10) && !bitmap.get(11, 10));
        assert!(bitmap.get(12, 10), "outside the eraser");
        bitmap.erase(centre, centre, 5.0);
        assert_eq!(bitmap.to_mask(), None);
    }

    #[test]
    fn touching_masks_overlap_and_merge_into_their_union() {
        let (left, right, far) = (dab((10.5, 10.5), 2.0), dab((13.5, 10.5), 2.0), dab((40.5, 40.5), 2.0));
        assert!(left.overlaps(&right, left.area));
        assert!(!left.overlaps(&right, PixelRect { left: 0, top: 0, width: 11, height: 80 }), "they meet right of x = 11");
        assert!(!left.overlaps(&far, left.area));
        let mut merged = left.clone();
        merged.merge(&right, IMAGE);
        merged.merge(&far, IMAGE);
        let mut painted = left.clone();
        for at in [(13.5, 10.5), (40.5, 40.5)] {
            let at = Point::new(at.0, at.1);
            painted.paint(at, at, 2.0, IMAGE);
        }
        assert_eq!(merged.to_mask(), painted.to_mask());
    }

    fn from_rows(left: u32, top: u32, rows: &[&str]) -> Bitmap {
        let width = rows[0].len() as u32;
        let pixels = rows.iter().flat_map(|row| row.bytes().map(|b| u8::from(b == b'#'))).collect();
        Bitmap { area: PixelRect { left, top, width, height: rows.len() as u32 }, pixels }
    }

    fn rows(bitmap: &Bitmap) -> Vec<String> {
        bitmap.pixels.chunks(bitmap.area.width as usize).map(|row| row.iter().map(|&p| if p != 0 { '#' } else { '.' }).collect()).collect()
    }

    #[test]
    fn cleaning_removes_small_specks_and_fills_small_holes() {
        let bitmap = from_rows(0, 0, &[
            "#.......",
            "..#####.",
            "..#..##.",
            "..#..##.",
            "..#####.",
            "........",
        ]);
        assert_eq!(rows(&bitmap.clone().cleaned(5)), [
            "........",
            "..#####.",
            "..#####.",
            "..#####.",
            "..#####.",
            "........",
        ]);
        assert_eq!(rows(&bitmap.clone().cleaned(4))[2], "..#..##.");
        assert_eq!(bitmap.clone().cleaned(0), bitmap);
    }

    #[test]
    fn a_level_region_scales_up_to_image_pixels_inside_the_image() {
        // Level 1 pixels (1, 1) and (2, 3) on a 5 × 7 image.
        let level = from_rows(1, 1, &["#.", "..", ".#"]);
        let image = level.scaled_up(2, (5, 7));
        assert_eq!(image.bounds(), Some(PixelRect { left: 2, top: 2, width: 3, height: 5 }));
        assert!(image.get(2, 2) && image.get(3, 3) && image.get(4, 6));
        assert!(!image.get(4, 2) && !image.get(5, 6), "unset, or outside the image");
    }

    #[test]
    fn subtracting_clears_the_pixels_set_in_the_other() {
        let mut bitmap = from_rows(0, 0, &["####", "####"]);
        let cut = bitmap.subtract(&from_rows(1, 1, &["##", "##"]));
        assert_eq!(rows(&bitmap), ["####", "#..#"]);
        assert_eq!(cut, PixelRect { left: 1, top: 1, width: 2, height: 1 });
    }
}
