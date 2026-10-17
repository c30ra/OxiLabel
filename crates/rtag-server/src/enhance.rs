//! The enhancement filters' pixel work: `flatten_illumination`, `enhance_for_tagging` and
//! `enhance_v2` of the author's `defect_detection/seg_prep/common.py`, step by step, with the
//! OpenCV operations they use (Gaussian and median blur, area and bilinear resizing, gray
//! conversion, CLAHE) done the way OpenCV does them: same kernel sizes, borders, rounding.
//!
//! One shortcut: the blurs that estimate smooth things (the lighting, `enhance_v2`'s local
//! contrast) can be hundreds of pixels wide on large photos or with large parameters, too slow at
//! full size, so they run on a copy shrunk until the blur is about [`FINE_SIGMA`] pixels wide, and
//! are scaled back up. What they estimate is smooth, so this changes the result by about a gray
//! level; with the default parameters only `flatten_illumination`'s blur is this wide.
//!
//! One difference: `flatten_illumination` takes the lighting's mean with numpy in 32-bit floats,
//! which drifts on large photos (on a 7008 × 4672 one its output comes out a few gray levels
//! darker); here the mean is exact.

use rtag_core::filter::{Filter, Preset};

/// Below this many pixels of blur radius the lighting blur runs at full size.
const FINE_SIGMA: f64 = 40.0;
/// `enhance_v2`'s `lowres`: the lighting and local contrast are found at a quarter of the size.
const LOWRES: usize = 4;

/// An 8-bit image: gray (1 channel) or RGB (3), row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
}

/// `image` through `filter`: RGB stays RGB for flattening, the other presets give gray.
pub fn apply(filter: &Filter, image: &Pixels) -> Pixels {
    match filter.preset() {
        Preset::Flatten => flatten_illumination(image, filter.value("sigma_frac")),
        Preset::Clahe => {
            let flat = flatten_illumination(image, filter.value("sigma_frac"));
            clahe(&gray(&flat), filter.value("clip"), filter.value("tiles") as usize)
        }
        Preset::EnhanceV2 => enhance_v2(
            &gray(image),
            filter.value("bg_frac"),
            filter.value("detail_sigma"),
            filter.value("norm_frac"),
            filter.value("gain"),
            filter.value("clip"),
        ),
    }
}

/// A float image of one channel.
#[derive(Debug, Clone, PartialEq)]
struct Plane {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Plane {
    fn new(width: usize, height: usize) -> Plane {
        Plane { width, height, data: vec![0.0; width * height] }
    }

    fn row(&self, y: usize) -> &[f32] {
        &self.data[y * self.width..(y + 1) * self.width]
    }

    fn map(&self, f: impl Fn(f32) -> f32 + Sync) -> Plane {
        let mut out = Plane::new(self.width, self.height);
        in_parallel(&mut out, |y, row| row.iter_mut().zip(self.row(y)).for_each(|(o, v)| *o = f(*v)));
        out
    }

    fn zip(&self, other: &Plane, f: impl Fn(f32, f32) -> f32 + Sync) -> Plane {
        let mut out = Plane::new(self.width, self.height);
        in_parallel(&mut out, |y, row| {
            for ((o, a), b) in row.iter_mut().zip(self.row(y)).zip(other.row(y)) {
                *o = f(*a, *b);
            }
        });
        out
    }
}

/// Fills `out` row by row (`fill(y, row)`), spreading the rows over the cores.
fn in_parallel(out: &mut Plane, fill: impl Fn(usize, &mut [f32]) + Sync) {
    let width = out.width.max(1);
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let band = out.height.div_ceil(threads).max(16);
    std::thread::scope(|scope| {
        for (index, rows) in out.data.chunks_mut(band * width).enumerate() {
            let fill = &fill;
            scope.spawn(move || {
                for (offset, row) in rows.chunks_mut(width).enumerate() {
                    fill(index * band + offset, row);
                }
            });
        }
    });
}

/// One channel of `image` as floats.
fn channel(image: &Pixels, channel: usize) -> Plane {
    let data = image.data.iter().skip(channel).step_by(image.channels).map(|&v| f32::from(v)).collect();
    Plane { width: image.width, height: image.height, data }
}

/// `values` clipped to 0–255 and cut to whole numbers, as numpy's `clip(…).astype(uint8)`.
fn to_bytes(value: f32) -> u8 {
    value.clamp(0.0, 255.0) as u8
}

/// `saturate_cast<uchar>` of a float: rounded half to even, clipped.
fn round_to_byte(value: f32) -> u8 {
    value.round_ties_even().clamp(0.0, 255.0) as u8
}

/// OpenCV's `cvtColor(…, COLOR_BGR2GRAY)` on 8 bits (15-bit fixed point), from RGB.
fn gray(image: &Pixels) -> Pixels {
    if image.channels == 1 {
        return image.clone();
    }
    let data = image
        .data
        .chunks_exact(image.channels)
        .map(|p| ((u32::from(p[0]) * 9798 + u32::from(p[1]) * 19235 + u32::from(p[2]) * 3735 + (1 << 14)) >> 15) as u8)
        .collect();
    Pixels { width: image.width, height: image.height, channels: 1, data }
}

/// OpenCV's BORDER_REFLECT_101 index (`gfedcb|abcdefgh|gfedcba`), for any distance outside.
fn reflect(mut index: isize, length: usize) -> usize {
    let length = length as isize;
    if length == 1 {
        return 0;
    }
    while index < 0 || index >= length {
        index = if index < 0 { -index } else { 2 * length - 2 - index };
    }
    index as usize
}

/// `cv2.GaussianBlur(plane, (0, 0), sigma)` on floats: a kernel of `round(8σ + 1) | 1` taps,
/// reflected borders.
fn gaussian_blur(plane: &Plane, sigma: f64) -> Plane {
    let taps = ((sigma * 8.0 + 1.0).round_ties_even() as usize) | 1;
    let centre = (taps / 2) as isize;
    let weights: Vec<f64> = (0..taps).map(|i| (-0.5 * ((i as isize - centre) as f64).powi(2) / (sigma * sigma)).exp()).collect();
    let total: f64 = weights.iter().sum();
    let kernel: Vec<f32> = weights.iter().map(|w| (w / total) as f32).collect();
    let (width, height) = (plane.width, plane.height);

    let mut across = Plane::new(width, height);
    in_parallel(&mut across, |y, row| {
        let source = plane.row(y);
        let padded: Vec<f32> = (-centre..width as isize + centre).map(|x| source[reflect(x, width)]).collect();
        for (x, out) in row.iter_mut().enumerate() {
            *out = padded[x..x + taps].iter().zip(&kernel).map(|(v, k)| v * k).sum();
        }
    });
    let mut down = Plane::new(width, height);
    in_parallel(&mut down, |y, row| {
        for (tap, k) in kernel.iter().enumerate() {
            let source = across.row(reflect(y as isize + tap as isize - centre, height));
            row.iter_mut().zip(source).for_each(|(out, v)| *out += v * k);
        }
    });
    down
}

/// `cv2.medianBlur` on 8 bits: the median of each `size` × `size` square, the border repeated.
fn median_blur(image: &Pixels, size: usize) -> Pixels {
    let (width, height) = (image.width, image.height);
    if size <= 1 {
        return image.clone();
    }
    let radius = (size / 2) as isize;
    let at = |x: isize, y: isize| image.data[y.clamp(0, height as isize - 1) as usize * width + x.clamp(0, width as isize - 1) as usize];
    let rank = size * size / 2;
    let mut out = Plane::new(width, height);
    // Huang's sliding histogram along each row.
    in_parallel(&mut out, |y, row| {
        let y = y as isize;
        let mut histogram = [0u32; 256];
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                histogram[usize::from(at(dx, y + dy))] += 1;
            }
        }
        for (x, out) in row.iter_mut().enumerate() {
            let x = x as isize;
            if x > 0 {
                for dy in -radius..=radius {
                    histogram[usize::from(at(x - radius - 1, y + dy))] -= 1;
                    histogram[usize::from(at(x + radius, y + dy))] += 1;
                }
            }
            let mut seen = 0;
            *out = histogram
                .iter()
                .position(|&count| {
                    seen += count as usize;
                    seen > rank
                })
                .unwrap_or(255) as f32;
        }
    });
    Pixels { width, height, channels: 1, data: out.data.iter().map(|&v| v as u8).collect() }
}

/// The source pixels (index, weight) that make up each of `target` pixels when `source` pixels are
/// shrunk to them, as OpenCV's INTER_AREA computes them.
fn area_weights(source: usize, target: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = source as f64 / target as f64;
    (0..target)
        .map(|d| {
            let start = d as f64 * scale;
            let end = start + scale;
            let cell = scale.min(source as f64 - start);
            let last = (end.floor() as usize).min(source - 1);
            let first = (start.ceil() as usize).min(last);
            let mut weights = Vec::new();
            if first as f64 - start > 1e-3 {
                weights.push((first - 1, ((first as f64 - start) / cell) as f32));
            }
            weights.extend((first..last).map(|s| (s, (1.0 / cell) as f32)));
            if end - last as f64 > 1e-3 {
                weights.push((last, ((end - last as f64).min(1.0).min(cell) / cell) as f32));
            }
            weights
        })
        .collect()
}

/// `cv2.resize(…, INTER_AREA)` to a smaller size: each pixel the average of the area it covers.
fn shrink(plane: &Plane, width: usize, height: usize) -> Plane {
    let (columns, rows) = (area_weights(plane.width, width), area_weights(plane.height, height));
    let mut out = Plane::new(width, height);
    in_parallel(&mut out, |y, row| {
        let mut line = vec![0.0f32; plane.width];
        for &(source, weight) in &rows[y] {
            line.iter_mut().zip(plane.row(source)).for_each(|(l, v)| *l += v * weight);
        }
        for (out, weights) in row.iter_mut().zip(&columns) {
            *out = weights.iter().map(|&(source, weight)| line[source] * weight).sum();
        }
    });
    out
}

/// The two source pixels and the weight of the second for each of `target` pixels when `source`
/// pixels are stretched to them, as OpenCV's INTER_LINEAR (pixel centres aligned, edges held).
fn linear_weights(source: usize, target: usize) -> Vec<(usize, usize, f32)> {
    let scale = source as f64 / target as f64;
    (0..target)
        .map(|d| {
            let position = ((d as f64 + 0.5) * scale - 0.5) as f32;
            let index = position.floor();
            let (index, fraction) = if index < 0.0 {
                (0, 0.0)
            } else if index as usize >= source - 1 {
                (source - 1, 0.0)
            } else {
                (index as usize, position - index)
            };
            (index, (index + 1).min(source - 1), fraction)
        })
        .collect()
}

/// `cv2.resize(…, INTER_LINEAR)` on floats.
fn stretch(plane: &Plane, width: usize, height: usize) -> Plane {
    let (columns, rows) = (linear_weights(plane.width, width), linear_weights(plane.height, height));
    let across = |y: usize| -> Vec<f32> {
        let source = plane.row(y);
        columns.iter().map(|&(a, b, f)| source[a] * (1.0 - f) + source[b] * f).collect()
    };
    let mut out = Plane::new(width, height);
    in_parallel(&mut out, |y, row| {
        let (a, b, f) = rows[y];
        let (top, bottom) = (across(a), across(b));
        for ((out, t), b) in row.iter_mut().zip(&top).zip(&bottom) {
            *out = t * (1.0 - f) + b * f;
        }
    });
    out
}

/// numpy's `percentile` (linear between the two nearest ranks) of `values`.
fn percentile(values: &[f32], share: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    let position = share * (sorted.len() - 1) as f64;
    let (low, high) = (position.floor() as usize, position.ceil() as usize);
    f64::from(sorted[low]) + (f64::from(sorted[high]) - f64::from(sorted[low])) * (position - low as f64)
}

fn median(values: &[f32]) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    let half = sorted.len() / 2;
    if sorted.len() % 2 == 1 { sorted[half] } else { (sorted[half - 1] + sorted[half]) / 2.0 }
}

/// A Gaussian blur of `plane` of something smooth (lighting, local contrast), at full size while
/// `sigma` is at most [`FINE_SIGMA`], else on a shrunk copy (see the module notes).
fn lighting_blur(plane: &Plane, sigma: f64) -> Plane {
    let factor = (sigma / FINE_SIGMA).floor() as usize;
    if factor < 2 {
        return gaussian_blur(plane, sigma);
    }
    let (width, height) = (plane.width.div_ceil(factor), plane.height.div_ceil(factor));
    let small = shrink(plane, width, height);
    let blurred = gaussian_blur(&small, sigma * width as f64 / plane.width as f64);
    stretch(&blurred, plane.width, plane.height)
}

/// `flatten_illumination`: each channel divided by its blurred self (the lighting), times the
/// lighting's mean.
fn flatten_illumination(image: &Pixels, sigma_frac: f64) -> Pixels {
    let sigma = (sigma_frac * image.width.max(image.height) as f64).max(1.0);
    let channels: Vec<Plane> = (0..image.channels)
        .map(|c| {
            let lifted = channel(image, c).map(|v| v + 1.0);
            let lighting = lighting_blur(&lifted, sigma);
            let mean = (lighting.data.iter().map(|&v| f64::from(v)).sum::<f64>() / lighting.data.len() as f64) as f32;
            lifted.zip(&lighting, |v, l| v / l * mean)
        })
        .collect();
    let mut data = vec![0u8; image.data.len()];
    for (c, plane) in channels.iter().enumerate() {
        for (i, v) in plane.data.iter().enumerate() {
            data[i * image.channels + c] = to_bytes(*v);
        }
    }
    Pixels { data, ..*image }
}

/// OpenCV's CLAHE (`createCLAHE(clipLimit, (tiles, tiles)).apply`) on a gray image: a clipped,
/// equalised histogram per tile, blended between neighbouring tiles.
fn clahe(image: &Pixels, clip: f64, tiles: usize) -> Pixels {
    let (width, height) = (image.width, image.height);
    // Unless tiles divide both sides, the image is padded (reflected) on the right and bottom by
    // `tiles - side % tiles`: a side that divides gets a whole `tiles` more, as in OpenCV.
    let divides = width % tiles == 0 && height % tiles == 0;
    let padded = |side: usize| if divides { side } else { side + tiles - side % tiles };
    let (tile_width, tile_height) = (padded(width) / tiles, padded(height) / tiles);
    let tile_area = tile_width * tile_height;
    let limit = if clip > 0.0 { ((clip * tile_area as f64 / 256.0) as u32).max(1) } else { 0 };
    let scale = 255.0f32 / tile_area as f32;

    let mut luts = vec![[0u8; 256]; tiles * tiles];
    for (index, lut) in luts.iter_mut().enumerate() {
        let (tx, ty) = (index % tiles, index / tiles);
        let mut histogram = [0u32; 256];
        for y in ty * tile_height..(ty + 1) * tile_height {
            let row = reflect(y as isize, height) * width;
            for x in tx * tile_width..(tx + 1) * tile_width {
                histogram[usize::from(image.data[row + reflect(x as isize, width)])] += 1;
            }
        }
        if limit > 0 {
            let mut clipped = 0;
            for count in histogram.iter_mut().filter(|count| **count > limit) {
                clipped += *count - limit;
                *count = limit;
            }
            let batch = clipped / 256;
            let mut residual = clipped - batch * 256;
            histogram.iter_mut().for_each(|count| *count += batch);
            if residual > 0 {
                let step = (256 / residual as usize).max(1);
                let mut i = 0;
                while i < 256 && residual > 0 {
                    histogram[i] += 1;
                    i += step;
                    residual -= 1;
                }
            }
        }
        let mut sum = 0;
        for (value, count) in lut.iter_mut().zip(histogram) {
            sum += count;
            *value = round_to_byte(sum as f32 * scale);
        }
    }

    // Each pixel blends the tables of the (up to) four tiles whose centres surround it.
    let neighbours = |position: usize, tile: usize| {
        let at = position as f32 * (1.0 / tile as f32) - 0.5;
        let first = at.floor();
        let weight = at - first;
        let first = first as isize;
        (first.max(0) as usize, ((first + 1) as usize).min(tiles - 1), weight)
    };
    let columns: Vec<_> = (0..width).map(|x| neighbours(x, tile_width)).collect();
    let mut out = Plane::new(width, height);
    in_parallel(&mut out, |y, row| {
        let (top, bottom, down) = neighbours(y, tile_height);
        for (x, out) in row.iter_mut().enumerate() {
            let value = usize::from(image.data[y * width + x]);
            let (left, right, across) = columns[x];
            let blend = |ty: usize| f32::from(luts[ty * tiles + left][value]) * (1.0 - across) + f32::from(luts[ty * tiles + right][value]) * across;
            *out = blend(top) * (1.0 - down) + blend(bottom) * down;
        }
    });
    Pixels { width, height, channels: 1, data: out.data.iter().map(|&v| round_to_byte(v)).collect() }
}

/// `enhance_v2` on a gray image (see `common.py` for the reasoning of each step).
fn enhance_v2(image: &Pixels, bg_frac: f64, detail_sigma: f64, norm_frac: f64, gain: f64, clip: f64) -> Pixels {
    let (width, height) = (image.width, image.height);
    let (small_width, small_height) = ((width / LOWRES).max(1), (height / LOWRES).max(1));
    let original = channel(image, 0);
    let small = shrink(&original, small_width, small_height);
    let small = Pixels { width: small_width, height: small_height, channels: 1, data: small.data.iter().map(|&v| round_to_byte(v)).collect() };

    // The lighting: a large median (hot spots do not pull it up), smoothed; dark areas held up.
    let size = ((bg_frac * width as f64 / LOWRES as f64) as usize) | 1;
    let lighting = lighting_blur(&channel(&median_blur(&small, size), 0), size as f64 / 2.0);
    let floor = (0.35 * percentile(&lighting.data, 0.9)) as f32;
    let lighting = stretch(&lighting.map(|v| v.max(floor) + 1.0), width, height);

    // Flattened, bright values compressed softly.
    let flat = original.zip(&lighting, |v, l| {
        let flat = v / l;
        if flat > 1.0 { 1.0 + ((flat - 1.0) / 0.4).tanh() * 0.4 } else { flat }
    });
    let blurred = gaussian_blur(&flat, detail_sigma);
    let detail = flat.zip(&blurred, |f, b| f - b);

    // Detail over a robust local contrast.
    let sample: Vec<f32> = (0..height).step_by(4).flat_map(|y| detail.row(y).iter().step_by(4).copied()).collect();
    let centre = median(&sample);
    let mad = median(&sample.iter().map(|v| (v - centre).abs()).collect::<Vec<_>>()) + 1e-4;
    let squared = detail.map(|d| {
        let clipped = d.clamp(-6.0 * mad, 6.0 * mad);
        clipped * clipped
    });
    let spread = lighting_blur(&shrink(&squared, small_width, small_height), norm_frac * width as f64 / LOWRES as f64).map(f32::sqrt);
    let local = stretch(&spread, width, height);

    let (clip, gain) = (clip as f32, gain as f32);
    let mut out = Plane::new(width, height);
    in_parallel(&mut out, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let i = y * width + x;
            let norm = (detail.data[i] / (local.data[i] + mad)).clamp(-clip, clip) / clip;
            let base = (flat.data[i] - 1.0) * 1.5;
            *out = 128.0 + 100.0 * base.clamp(-1.0, 0.6) + 55.0 * gain * 2.0 * norm;
        }
    });
    Pixels { width, height, channels: 1, data: out.data.iter().map(|&v| to_bytes(v)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_reflect_without_repeating_the_edge() {
        let indices: Vec<usize> = (-4..8).map(|i| reflect(i, 4)).collect();
        assert_eq!(indices, [2, 3, 2, 1, 0, 1, 2, 3, 2, 1, 0, 1]);
        assert_eq!(reflect(-5, 1), 0);
    }

    #[test]
    fn resizing_matches_opencv() {
        // cv2.resize(arange(10) * 10, (25, 1), INTER_LINEAR)
        let row = Plane { width: 10, height: 1, data: (0..10).map(|v| v as f32 * 10.0).collect() };
        let stretched = stretch(&row, 25, 1);
        assert_eq!(&stretched.data[..4], &[0.0, 1.0, 5.0, 9.0]);
        assert_eq!(&stretched.data[23..], &[89.0, 90.0]);
        // Shrinking 4 to 2 averages pairs; 5 to 2 shares the middle pixel.
        let row = Plane { width: 5, height: 1, data: vec![0.0, 2.0, 4.0, 6.0, 8.0] };
        assert_eq!(shrink(&Plane { width: 4, height: 1, data: vec![0.0, 2.0, 4.0, 6.0] }, 2, 1).data, [1.0, 5.0]);
        let shrunk = shrink(&row, 2, 1).data;
        assert!((shrunk[0] - 1.6).abs() < 1e-5 && (shrunk[1] - 6.4).abs() < 1e-5, "{shrunk:?}");
    }

    #[test]
    fn the_median_ignores_a_bright_speck() {
        let mut data = vec![10u8; 25];
        data[12] = 250;
        let image = Pixels { width: 5, height: 5, channels: 1, data };
        assert!(median_blur(&image, 3).data.iter().all(|&v| v == 10));
    }
}
