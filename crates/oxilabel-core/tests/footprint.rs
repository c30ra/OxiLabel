//! The export footprint region: the pixels a polyline or points shape becomes in exports, which
//! the editor's footprint ghost shows too.

use std::collections::BTreeMap;

use oxilabel_core::{
    Footprint, Label, LabelDefinition, PointShape, ShapeType,
    annotation::Shape,
    export::{ExportRequest, ImageSelection},
    geometry::{Point, Rect},
    mask::{Bitmap, PixelRect},
};

const IMAGE: (u32, u32) = (100, 80);

fn footprint(line_width: f64, point_radius: f64, point_shape: PointShape) -> Footprint {
    Footprint { line_width, point_radius, point_shape }
}

fn polyline(points: &[(f64, f64)]) -> Shape {
    Shape::Polyline { points: points.iter().map(|&(x, y)| Point::new(x, y)).collect() }
}

fn points(points: &[(f64, f64)]) -> Shape {
    Shape::Points { points: points.iter().map(|&(x, y)| Point::new(x, y)).collect() }
}

/// The bounds and the number of the set pixels.
fn extent(region: &Bitmap) -> (PixelRect, usize) {
    (region.bounds().expect("some pixels"), region.pixels.iter().filter(|&&p| p != 0).count())
}

fn rect(left: u32, top: u32, width: u32, height: u32) -> PixelRect {
    PixelRect { left, top, width, height }
}

#[test]
fn a_polyline_covers_a_round_ended_band_2_ceil_half_width_plus_1_pixels_across() {
    let region = footprint(4.0, 8.0, PointShape::Disc).region(&polyline(&[(10.0, 10.0), (30.0, 10.0)]), IMAGE).unwrap();
    // Width 4: 5 rows, the pixels whose centres lie within 2.5 of the line between the centres of
    // pixels (10, 10) and (30, 10): rows 9 to 11 run from x 8 to 32, rows 8 and 12 from 9 to 31.
    assert_eq!(extent(&region), (rect(8, 8, 25, 5), 3 * 25 + 2 * 23));
    assert!(region.get(8, 10) && !region.get(8, 8) && region.get(9, 8) && !region.get(10, 13));
}

#[test]
fn coordinates_snap_to_the_pixel_they_fall_in() {
    let exact = footprint(4.0, 8.0, PointShape::Disc).region(&polyline(&[(10.0, 10.0), (30.0, 10.0)]), IMAGE).unwrap();
    let off = footprint(4.0, 8.0, PointShape::Disc).region(&polyline(&[(10.3, 9.8), (29.6, 10.2)]), IMAGE).unwrap();
    assert_eq!(extent(&off), extent(&exact));
}

#[test]
fn a_one_point_polyline_is_a_dot() {
    let region = footprint(4.0, 8.0, PointShape::Disc).region(&polyline(&[(50.0, 50.0)]), IMAGE).unwrap();
    // The pixels whose centres lie within 2.5 of pixel (50, 50)'s: rows of 3, 5, 5, 5 and 3.
    assert_eq!(extent(&region), (rect(48, 48, 5, 5), 21));
}

#[test]
fn a_round_point_covers_the_pixels_whose_centres_are_within_its_whole_radius() {
    // Radius 3.7 counts as 3: rows of 1, 5, 5, 7, 5, 5 and 1 pixels around pixel (20, 20).
    let region = footprint(4.0, 3.7, PointShape::Disc).region(&points(&[(20.0, 20.0), (60.0, 40.0)]), IMAGE).unwrap();
    assert_eq!(extent(&region), (rect(17, 17, 47, 27), 2 * 29));
    assert!(region.get(20, 17) && !region.get(18, 17) && region.get(63, 40));
}

#[test]
fn a_square_point_covers_a_square_twice_its_radius_and_one_pixel_across_inside_the_image() {
    let region = footprint(4.0, 3.0, PointShape::Square).region(&points(&[(20.0, 20.0)]), IMAGE).unwrap();
    assert_eq!(extent(&region), (rect(17, 17, 7, 7), 49));
    let corner = footprint(4.0, 3.0, PointShape::Square).region(&points(&[(0.0, 0.0)]), IMAGE).unwrap();
    assert_eq!(extent(&corner), (rect(0, 0, 4, 4), 16));
}

#[test]
fn shapes_other_than_lines_and_points_have_no_footprint() {
    let box_ = Shape::Rectangle(Rect { min: Point::new(1.0, 1.0), max: Point::new(9.0, 9.0) });
    assert_eq!(Footprint::default().region(&box_, IMAGE), None);
    assert_eq!(Footprint::default().region(&Shape::Tag, IMAGE), None);
}

fn label(id: i64, line_width: f64) -> Label {
    let definition = LabelDefinition {
        name: format!("label {id}"),
        color: "#ff0000".into(),
        hotkey: None,
        shape_types: vec![ShapeType::Polyline],
        ignore: false,
        attributes: vec![],
        footprint: footprint(line_width, 8.0, PointShape::Disc),
        show_footprint: true,
        constraints: Default::default(),
    };
    Label::new(id, definition)
}

/// A request whose footprint for label 1 has lines `line_width` wide.
fn request_with_width(line_width: f64) -> ExportRequest {
    ExportRequest {
        format: "yolo-segment".into(),
        images: ImageSelection::All,
        include_images: None,
        split: Default::default(),
        footprints: BTreeMap::from([(1, footprint(line_width, 8.0, PointShape::Disc))]),
        footprint_bounds: true,
    }
}

#[test]
fn an_export_can_override_a_labels_line_width() {
    let line = polyline(&[(10.0, 40.0), (90.0, 40.0)]);
    let request = request_with_width(10.0);
    let height = |label: &Label| extent(&request.footprint_of(label).unwrap().region(&line, IMAGE).unwrap()).0.height;
    // Width 10 for this export: 11 rows; label 2 keeps its own width 4: 5 rows.
    assert_eq!((height(&label(1, 4.0)), height(&label(2, 4.0))), (11, 5));

    let unsound = request_with_width(0.0);
    assert_eq!(unsound.footprint_of(&label(1, 4.0)), Err("label 1: line width and point radius must be above zero".to_owned()));
}
