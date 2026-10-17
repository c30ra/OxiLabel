//! YOLO segment and YOLO OBB on one 200 × 100 image with every kind of shape.

use rtag_core::{
    Label, ShapeType,
    annotation::{Annotation, Shape},
    ellipse::Ellipse,
    geometry::{Point, Rect, polygon_area},
    mask::Mask,
    oriented::OrientedRect,
};
use rtag_formats::{ExportImage, ExportInput, ImageMode, MemorySink, Split, YoloObb, YoloSegment, exporter, run_export};

const ALL_TRAIN: Split = Split { train: 100, val: 0, test: 0 };

fn label(id: i64, name: &str, ignore: bool, line_width: f64, point_radius: f64) -> Label {
    Label {
        id,
        name: name.into(),
        color: "#ffffff".into(),
        hotkey: None,
        shape_types: ShapeType::ALL.to_vec(),
        ignore,
        attributes: vec![],
        line_width,
        point_radius,
    }
}

/// A 20 × 10 ellipse turned by 45°.
const ELLIPSE: Ellipse = Ellipse { centre: Point { x: 150.0, y: 30.0 }, rx: 10.0, ry: 5.0, angle: std::f64::consts::FRAC_PI_4 };

fn points(list: &[(f64, f64)]) -> Vec<Point> {
    list.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

/// One image: a polygon, a box, a turned box, a polyline, a point, a brush mask, a turned ellipse, a bent
/// polyline, two points, a one-pixel mask, a one-point polyline, a tag and an ignore-label polygon.
fn input() -> ExportInput {
    let annotations = vec![
        Annotation::manual(2, Shape::Polygon { points: points(&[(20.0, 10.0), (60.0, 10.0), (40.0, 50.0)]) }),
        Annotation::manual(1, Shape::Rectangle(Rect::new(10.0, 10.0, 30.0, 20.0))),
        Annotation::manual(2, Shape::OrientedRectangle(OrientedRect { centre: Point::new(100.0, 50.0), width: 40.0, height: 20.0, angle: 0.0 })),
        Annotation::manual(1, Shape::Polyline { points: points(&[(50.0, 80.0), (150.0, 80.0)]) }),
        Annotation::manual(1, Shape::Points { points: points(&[(180.0, 20.0)]) }),
        Annotation::manual(2, Shape::Mask(Mask { left: 190, top: 90, width: 3, height: 2, rle: vec![0, 6] })),
        Annotation::manual(1, Shape::Ellipse(ELLIPSE)),
        Annotation::manual(2, Shape::Polyline { points: points(&[(20.0, 60.0), (60.0, 90.0), (100.0, 60.0)]) }),
        Annotation::manual(1, Shape::Points { points: points(&[(5.0, 95.0), (15.0, 95.0)]) }),
        Annotation::manual(2, Shape::Mask(Mask { left: 199, top: 0, width: 1, height: 1, rle: vec![0, 1] })),
        Annotation::manual(1, Shape::Polyline { points: points(&[(120.0, 90.0)]) }),
        Annotation::manual(2, Shape::Tag),
        Annotation::manual(3, Shape::Polygon { points: points(&[(0.0, 0.0), (5.0, 0.0), (0.0, 5.0)]) }),
    ];
    ExportInput {
        project: "Shapes".into(),
        labels: vec![label(1, "crack", false, 4.0, 3.0), label(2, "finger", false, 7.0, 8.0), label(3, "ignore", true, 7.0, 8.0)],
        images: vec![ExportImage {
            path: "cells/a.png".into(),
            source: "/images/cells/a.png".into(),
            hash: "hash-a".into(),
            width: 200,
            height: 100,
            annotations,
        }],
        split: ALL_TRAIN,
    }
}

/// The label file's lines as (class, points in image pixels).
fn lines(exporter: &dyn rtag_formats::Exporter) -> Vec<(i64, Vec<Point>)> {
    let mut sink = MemorySink::default();
    run_export(exporter, &input(), &mut sink, "now", ImageMode::Symlink).unwrap();
    let text = String::from_utf8(sink.files["labels/train/cells__a.txt"].clone()).unwrap();
    assert!(sink.files["data.yaml"].starts_with(b"# YOLO"), "a data.yaml");
    text.lines()
        .map(|line| {
            let values: Vec<f64> = line.split(' ').map(|v| v.parse().unwrap()).collect();
            let corners = values[1..].chunks(2).map(|xy| Point::new(xy[0] * 200.0, xy[1] * 100.0)).collect();
            (values[0] as i64, corners)
        })
        .collect()
}

fn near(a: &[Point], b: &[Point]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 1e-4)
}

#[test]
fn segment_writes_exact_polygons_for_shapes_with_corners() {
    let lines = lines(&YoloSegment);
    assert_eq!(lines.len(), 12, "the two points are two objects; tag and ignore label left out: {lines:?}");
    assert_eq!(lines[0].0, 1);
    assert!(near(&lines[0].1, &points(&[(20.0, 10.0), (60.0, 10.0), (40.0, 50.0)])), "polygon");
    assert!(near(&lines[1].1, &points(&[(10.0, 10.0), (30.0, 10.0), (30.0, 20.0), (10.0, 20.0)])), "rectangle");
    assert!(near(&lines[2].1, &points(&[(80.0, 40.0), (120.0, 40.0), (120.0, 60.0), (80.0, 60.0)])), "turned box");
    assert!(near(&lines[5].1, &points(&[(190.0, 90.0), (193.0, 90.0), (193.0, 92.0), (190.0, 92.0)])), "brush mask outline: {:?}", lines[5].1);
}

/// Whether `polygon` covers about `pixels` pixels (as `cvat_to_masks.draw_shape` draws the shape,
/// see fixtures/cvat_to_masks_reference.py) within `share`, inside the pixel box `x` × `y`.
fn like_reference(polygon: &[Point], pixels: f64, share: f64, x: (f64, f64), y: (f64, f64)) -> bool {
    let area = polygon_area(polygon);
    let inside = polygon.iter().all(|p| (x.0..=x.1).contains(&p.x) && (y.0..=y.1).contains(&p.y));
    if !((area - pixels).abs() <= pixels * share && inside) {
        eprintln!("area {area} (reference {pixels}), points {polygon:?}");
    }
    (area - pixels).abs() <= pixels * share && inside
}

#[test]
fn segment_expands_polylines_and_points_like_cvat_to_masks() {
    let lines = lines(&YoloSegment);
    assert_eq!(lines[3].0, 0);
    assert!(like_reference(&lines[3].1, 513.0, 0.04, (48.0, 153.0), (78.0, 83.0)), "the straight line, 4 px");
    assert!(like_reference(&lines[4].1, 29.0, 0.2, (177.0, 184.0), (17.0, 24.0)), "the point, radius 3");
    // OpenCV's thick lines are not equally wide at every angle (for thickness 7, 8.2 to 9.2 pixels
    // per pixel of length, 9 on average); this line runs at its thinnest angle, rtag's is 9 wide.
    assert!(like_reference(&lines[7].1, 866.0, 0.15, (16.0, 105.0), (56.0, 95.0)), "the bent line, 7 px");
    assert_eq!((lines[8].0, lines[9].0), (0, 0), "each of the two points is its own object");
    assert!(near(&lines[10].1, &points(&[(199.0, 0.0), (200.0, 0.0), (200.0, 1.0), (199.0, 1.0)])), "a one-pixel mask: {:?}", lines[10].1);
    assert!(polygon_area(&lines[11].1) > 0.0, "a one-point polyline is a dot");
    let ellipse = &lines[6].1;
    assert!(ellipse.len() >= 16 && ellipse.iter().all(|p| ELLIPSE.contains(*p, 0.01) && !ELLIPSE.contains(*p, -0.01)), "ellipse {ellipse:?}");
}

#[test]
fn obb_writes_four_corners_per_shape() {
    let lines = lines(&YoloObb);
    assert_eq!(lines.len(), 12);
    assert!(lines.iter().all(|(_, corners)| corners.len() == 4));
    assert!(near(&lines[1].1, &points(&[(10.0, 10.0), (30.0, 10.0), (30.0, 20.0), (10.0, 20.0)])), "rectangle");
    assert!(near(&lines[2].1, &points(&[(80.0, 40.0), (120.0, 40.0), (120.0, 60.0), (80.0, 60.0)])), "turned box");
    let line_box = polygon_area(&lines[3].1);
    assert!((line_box - 525.0).abs() < 25.0, "the polyline's box is about 105 × 5: {line_box}");
    // The ellipse's box follows its axes: 20 × 10, turned by 45°.
    let ellipse = &lines[6].1;
    assert!((polygon_area(ellipse) - 200.0).abs() < 1.0, "{ellipse:?}");
    assert!((ellipse[1].distance(ellipse[0]) - 20.0).abs() < 0.01 && ((ellipse[1].y - ellipse[0].y) - (ellipse[1].x - ellipse[0].x)).abs() < 0.01, "{ellipse:?}");
    assert!(near(&lines[10].1, &points(&[(199.0, 0.0), (200.0, 0.0), (200.0, 1.0), (199.0, 1.0)])), "the one-pixel mask's box");
    let triangle = polygon_area(&lines[0].1);
    assert!((triangle - 1600.0).abs() < 1e-6, "the triangle's tightest box is 40 × 40: {triangle} {:?}", lines[0].1);
}

#[test]
fn both_are_listed_exporters() {
    assert_eq!(exporter("yolo-segment").unwrap().name(), "YOLO segment");
    assert_eq!(exporter("yolo-obb").unwrap().name(), "YOLO OBB");
}
