//! The enhancement filters against the author's `common.py`: the expected images in
//! `fixtures/filters` are its output on a synthetic photo (made by `fixtures/filters/reference.py`).

use std::path::Path;

use rtag_core::filter::{Filter, Preset};
use rtag_server::enhance::{Pixels, apply};

fn load(name: &str) -> Pixels {
    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filters").join(name)).expect("fixture");
    let channels = if image.color().has_color() { 3 } else { 1 };
    let data = if channels == 3 { image.to_rgb8().into_raw() } else { image.to_luma8().into_raw() };
    Pixels { width: image.width() as usize, height: image.height() as usize, channels, data }
}

/// Mean difference and the share of values more than 2 gray levels off.
fn compare(filter: Filter, expected: &str) -> (f64, f64) {
    let (actual, expected) = (apply(&filter, &load("input.png")), load(expected));
    assert_eq!((actual.width, actual.height, actual.channels), (expected.width, expected.height, expected.channels));
    let differences: Vec<f64> = actual.data.iter().zip(&expected.data).map(|(a, e)| (f64::from(*a) - f64::from(*e)).abs()).collect();
    let mean = differences.iter().sum::<f64>() / differences.len() as f64;
    let off = differences.iter().filter(|d| **d > 2.0).count() as f64 / differences.len() as f64;
    println!("{expected:?} mean {mean:.3} off {off:.4} max {}", differences.iter().cloned().fold(0.0, f64::max), expected = filter.key());
    (mean, off)
}

fn assert_close(filter: Filter, expected: &str) {
    let (mean, off) = compare(filter, expected);
    assert!(mean < 0.1 && off < 0.005, "{expected}: mean difference {mean:.3}, {:.2}% more than 2 levels off", off * 100.0);
}

#[test]
fn flatten_matches_flatten_illumination() {
    assert_close(Filter::new(Preset::Flatten), "flatten.png");
    // A blur wide enough to run on a shrunk copy.
    assert_close(Filter::with_values(Preset::Flatten, &[0.45]), "flatten_wide.png");
}

#[test]
fn clahe_matches_enhance_for_tagging() {
    assert_close(Filter::new(Preset::Clahe), "clahe.png");
    assert_close(Filter::with_values(Preset::Clahe, &[0.1, 1.5, 5.0]), "clahe_options.png");
}

#[test]
fn v2_matches_enhance_v2() {
    assert_close(Filter::new(Preset::EnhanceV2), "v2.png");
    assert_close(Filter::with_values(Preset::EnhanceV2, &[0.08, 3.0, 0.05, 1.2, 2.0]), "v2_options.png");
}
