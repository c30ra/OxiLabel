use rtag_core::{
    ShapeType,
    annotation::Shape,
    attribute::{AttributeDefinition, AttributeKind},
    ellipse::Ellipse,
    geometry::{Point, Rect},
    mask::Mask,
};
use rtag_formats::cvat::{CvatAnnotation, parse_cvat};

/// Two images from the author's `defect_detection` project (CVAT for images 1.1), trimmed and
/// renamed: masks, points, ellipses, polylines, boxes (one rotated) and a polygon.
const DEFECT_DETECTION: &str = include_str!("fixtures/cvat/defect_detection.xml");

fn kinds(annotations: &[CvatAnnotation]) -> Vec<(ShapeType, usize)> {
    let mut counts: Vec<(ShapeType, usize)> = Vec::new();
    for annotation in annotations {
        let kind = annotation.shape.shape_type();
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
    }
    counts.sort_by_key(|(kind, _)| ShapeType::ALL.iter().position(|k| k == kind));
    counts
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_defect_detection_export_gives_its_labels_images_and_shapes() {
    let export = parse_cvat(DEFECT_DETECTION).unwrap();
    let labels: Vec<_> = export.labels.iter().map(|l| (l.name.as_str(), l.color.as_deref())).collect();
    assert_eq!(labels, [
        ("crack", Some("#e6194b")),
        ("line_interruption", Some("#f58231")),
        ("surface_defect", Some("#ffe119")),
        ("shunt_top", Some("#3cb44b")),
        ("finger_delamination", Some("#911eb4")),
        ("chip", Some("#46f0f0")),
        ("ignore", Some("#4363d8")),
        ("shunt_middle", Some("#008080")),
    ]);
    // CVAT's label type "polyline" allows only polylines, "any" every shape.
    assert_eq!(export.labels[0].shape_types, [ShapeType::Polyline]);
    assert_eq!(export.labels[1].shape_types, ShapeType::ALL);

    let images: Vec<_> = export.images.iter().map(|i| (i.name.as_str(), i.width, i.height)).collect();
    assert_eq!(images, [("image1.jpg", 2048, 941), ("image2.jpg", 2048, 960)]);
    use ShapeType::*;
    assert_eq!(kinds(&export.images[0].annotations), [(Rectangle, 2), (Polyline, 2), (Points, 18), (Ellipse, 6), (Mask, 7)]);
    assert_eq!(kinds(&export.images[1].annotations), [(Rectangle, 1), (OrientedRectangle, 1), (Polygon, 1), (Polyline, 2), (Points, 25)]);
    assert!(export.images.iter().all(|image| image.unsupported.is_empty()));
}

#[test]
fn shapes_keep_their_cvat_geometry() {
    let export = parse_cvat(DEFECT_DETECTION).unwrap();
    let first = |image: usize, kind: ShapeType| export.images[image].annotations.iter().find(|a| a.shape.shape_type() == kind).unwrap();

    let chip = first(0, ShapeType::Rectangle);
    assert_eq!(chip.label, "chip");
    assert_eq!(chip.shape, Shape::Rectangle(Rect::new(932.91, 897.09, 945.56, 906.65)));
    assert_eq!(first(0, ShapeType::Points).shape, Shape::Points { points: vec![Point::new(1914.10, 750.61)] });
    let Shape::Polyline { points } = &first(0, ShapeType::Polyline).shape else { panic!() };
    assert_eq!((points.len(), points[0], points[4]), (5, Point::new(970.79, 904.27), Point::new(859.38, 759.95)));
    assert_eq!(
        first(0, ShapeType::Ellipse).shape,
        Shape::Ellipse(Ellipse { centre: Point::new(1613.55, 839.95), rx: 23.25, ry: 21.38, angle: 0.0 })
    );
    // CVAT masks use rtag's runs: the rows of their box, starting with unset pixels.
    let Shape::Mask(mask) = &first(0, ShapeType::Mask).shape else { panic!() };
    assert_eq!((mask.left, mask.top, mask.width, mask.height), (1800, 656, 14, 7));
    assert_eq!(mask.rle, [0, 5, 9, 7, 7, 8, 8, 9, 7, 10, 6, 8, 8, 6]);
    assert_eq!(mask.validate(2048, 941), Ok(()));

    // A box turned by 315.32° clockwise about its centre.
    let Shape::OrientedRectangle(turned) = first(1, ShapeType::OrientedRectangle).shape else { panic!() };
    assert!(close(turned.centre.x, (1700.41 + 1737.61) / 2.0) && close(turned.centre.y, (149.68 + 421.84) / 2.0));
    assert!(close(turned.width, 1737.61 - 1700.41) && close(turned.height, 421.84 - 149.68));
    assert!(close(turned.angle, 315.32_f64.to_radians()));
}

/// A hand-written export with attributes of every kind, an image tag, a turned ellipse, shapes
/// stacked by z_order, a label missing from the meta and shapes rtag has no type for.
const FEATURES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<annotations>
  <version>1.1</version>
  <meta>
    <task>
      <name>sample</name>
      <labels>
        <label>
          <name>defect</name>
          <color>#ff0000</color>
          <type>rectangle</type>
          <attributes>
            <attribute><name>occluded</name><mutable>False</mutable><input_type>checkbox</input_type><default_value>false</default_value><values>false
true</values></attribute>
            <attribute><name>severity</name><mutable>False</mutable><input_type>select</input_type><default_value>low</default_value><values>low
high</values></attribute>
            <attribute><name>side</name><mutable>False</mutable><input_type>radio</input_type><default_value>front</default_value><values>front
back</values></attribute>
            <attribute><name>length</name><mutable>False</mutable><input_type>number</input_type><default_value>2</default_value><values>0
10
1</values></attribute>
            <attribute><name>note</name><mutable>False</mutable><input_type>text</input_type><default_value></default_value><values></values></attribute>
          </attributes>
        </label>
        <label><name>good</name><color>#00ff00</color><type>tag</type><attributes></attributes></label>
      </labels>
    </task>
  </meta>
  <image id="0" name="parts/a.png" width="100" height="50">
    <tag label="good" source="manual"></tag>
    <box label="defect" source="manual" occluded="0" xtl="10" ytl="10" xbr="20" ybr="20" z_order="2">
      <attribute name="occluded">true</attribute>
      <attribute name="severity">high</attribute>
      <attribute name="length">7.5</attribute>
      <attribute name="note">a scratch</attribute>
    </box>
    <ellipse label="spot" source="manual" occluded="0" cx="50" cy="25" rx="10" ry="4" rotation="90" z_order="0"></ellipse>
    <cuboid label="defect" source="manual" occluded="0" z_order="0" xtl1="1" ytl1="1" xbl1="1" ybl1="2" xtr1="2" ytr1="1" xbr1="2" ybr1="2" xtl2="1" ytl2="1" xbl2="1" ybl2="2" xtr2="2" ytr2="1" xbr2="2" ybr2="2"></cuboid>
    <skeleton label="defect" source="manual" z_order="0"></skeleton>
  </image>
</annotations>
"#;

#[test]
fn attributes_tags_turned_ellipses_and_stacking_are_read() {
    let export = parse_cvat(FEATURES).unwrap();
    let defect = &export.labels[0];
    assert_eq!(defect.shape_types, [ShapeType::Rectangle, ShapeType::OrientedRectangle]);
    assert_eq!(export.labels[1].shape_types, [ShapeType::Tag]);
    let definition = |name: &str, kind| AttributeDefinition { name: name.into(), kind };
    assert_eq!(defect.attributes, [
        definition("occluded", AttributeKind::Checkbox { default: false }),
        definition("severity", AttributeKind::Select { options: vec!["low".into(), "high".into()], default: "low".into() }),
        definition("side", AttributeKind::Select { options: vec!["front".into(), "back".into()], default: "front".into() }),
        definition("length", AttributeKind::Number { min: Some(0.0), max: Some(10.0), default: 2.0 }),
        definition("note", AttributeKind::Text { default: String::new() }),
    ]);

    let image = &export.images[0];
    assert_eq!(image.name, "parts/a.png");
    // Lowest z_order first, so later shapes are drawn on top; the tag has no z_order.
    let order: Vec<_> = image.annotations.iter().map(|a| (a.label.as_str(), a.shape.shape_type())).collect();
    assert_eq!(order, [("good", ShapeType::Tag), ("spot", ShapeType::Ellipse), ("defect", ShapeType::Rectangle)]);
    assert_eq!(image.annotations[1].shape, Shape::Ellipse(Ellipse { centre: Point::new(50.0, 25.0), rx: 10.0, ry: 4.0, angle: 90_f64.to_radians() }));
    let values: Vec<_> = image.annotations[2].attributes.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
    assert_eq!(values, [("occluded", "true"), ("severity", "high"), ("length", "7.5"), ("note", "a scratch")]);
    assert_eq!(image.unsupported, ["cuboid", "skeleton"]);
}

#[test]
fn malformed_exports_are_refused_with_a_reason() {
    assert!(parse_cvat("not xml").unwrap_err().contains("XML"));
    assert!(parse_cvat("<annotations><version>2.0</version></annotations>").unwrap_err().contains("1.1"));
    let bad_box = r#"<annotations><version>1.1</version><image id="0" name="a.png" width="10" height="10"><box label="x" xtl="a" ytl="1" xbr="2" ybr="2"/></image></annotations>"#;
    let message = parse_cvat(bad_box).unwrap_err();
    assert!(message.contains("a.png") && message.contains("xtl"), "{message}");
}

#[test]
fn a_mask_from_cvat_is_an_rtag_mask() {
    let xml = r#"<annotations><version>1.1</version><image id="0" name="a.png" width="10" height="10">
      <mask label="x" source="manual" occluded="0" rle="1, 3, 1, 15, 1, 3, 1" left="3" top="4" width="5" height="5" z_order="0"></mask>
    </image></annotations>"#;
    let export = parse_cvat(xml).unwrap();
    assert_eq!(export.images[0].annotations[0].shape, Shape::Mask(Mask { left: 3, top: 4, width: 5, height: 5, rle: vec![1, 3, 1, 15, 1, 3, 1] }));
}
