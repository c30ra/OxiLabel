use std::io::{Cursor, Read};

use oxilabel_core::{
    Footprint, Label, PointShape, ShapeType,
    constraint::{Radii, ShapeConstraints, Size},
    annotation::{Annotation, Shape},
    attribute::{AttributeDefinition, AttributeKind, AttributeValue},
    ellipse::Ellipse,
    geometry::{Point, Rect},
    mask::Mask,
    oriented::OrientedRect,
};
use oxilabel_formats::archive::{ARCHIVE_VERSION, ArchiveImage, ArchiveJob, ArchiveReader, ArchiveWriter, ProjectArchive};

fn label(id: i64, name: &str, attributes: Vec<AttributeDefinition>) -> Label {
    Label {
        id,
        name: name.into(),
        color: "#ff8800".into(),
        hotkey: Some(id.to_string()),
        shape_types: ShapeType::ALL.to_vec(),
        ignore: id == 7,
        attributes,
        footprint: Footprint { line_width: 4.5, point_radius: 2.0, point_shape: PointShape::Square },
        show_footprint: id != 7,
        constraints: ShapeConstraints { rectangle: Some(Size { width: 12.0, height: 8.0 }), ellipse: Some(Radii { rx: 3.0, ry: 2.0 }), ..Default::default() },
    }
}

/// A project with every shape type, a tag, attribute values of every kind, the flags, a pending
/// pre-annotation, a job with an assignee and one without, and an image in no job.
fn project() -> ProjectArchive {
    let points = |list: &[(f64, f64)]| list.iter().map(|&(x, y)| Point::new(x, y)).collect::<Vec<_>>();
    let attributes = vec![
        AttributeDefinition { name: "occluded".into(), kind: AttributeKind::Checkbox { default: false } },
        AttributeDefinition { name: "severity".into(), kind: AttributeKind::Select { options: vec!["low".into(), "high".into()], default: "low".into() } },
        AttributeDefinition { name: "length".into(), kind: AttributeKind::Number { min: Some(0.0), max: None, default: 1.5 } },
        AttributeDefinition { name: "note".into(), kind: AttributeKind::Text { default: String::new() } },
    ];
    let mut with_values = Annotation::manual(3, Shape::Rectangle(Rect::new(1.0, 2.0, 30.5, 40.25)));
    with_values.attributes = [
        ("occluded".to_owned(), AttributeValue::Bool(true)),
        ("severity".to_owned(), AttributeValue::Text("high".into())),
        ("length".to_owned(), AttributeValue::Number(7.25)),
        ("note".to_owned(), AttributeValue::Text("a scratch, à côté".into())),
    ]
    .into();
    let pre_annotation = Annotation { source: Some("yolo v1".into()), confidence: Some(0.4375), pending: true, ..Annotation::manual(7, Shape::Polygon { points: points(&[(0.0, 0.0), (9.0, 0.0), (0.0, 9.0)]) }) };
    let annotations = vec![
        Annotation::manual(7, Shape::Tag),
        with_values,
        Annotation::manual(7, Shape::OrientedRectangle(OrientedRect { centre: Point::new(50.0, 50.0), width: 20.0, height: 10.0, angle: 0.785 })),
        pre_annotation,
        Annotation { hidden: true, ..Annotation::manual(7, Shape::Polyline { points: points(&[(1.0, 1.0), (5.5, 9.0), (12.0, 3.0)]) }) },
        Annotation { locked: true, ..Annotation::manual(7, Shape::Points { points: points(&[(4.0, 4.0), (8.0, 8.0)]) }) },
        Annotation::manual(7, Shape::Ellipse(Ellipse { centre: Point::new(60.0, 30.0), rx: 12.0, ry: 4.5, angle: -0.3 })),
        Annotation::manual(7, Shape::Mask(Mask { left: 10, top: 11, width: 3, height: 2, rle: vec![1, 4, 1] })),
    ];
    ProjectArchive {
        name: "EL defects".into(),
        next_label_id: 9,
        next_job_number: 4,
        labels: vec![label(3, "crack", attributes), label(7, "ignore", Vec::new())],
        jobs: vec![ArchiveJob { number: 1, assignee: Some("anna".into()) }, ArchiveJob { number: 3, assignee: None }],
        images: vec![
            ArchiveImage { path: "front/a.png".into(), hash: "aa11".into(), width: 100, height: 80, annotated: true, job: Some(1), file: None, annotations },
            ArchiveImage { path: "/photos/b.jpg".into(), hash: "bb22".into(), width: 640, height: 480, annotated: false, job: None, file: None, annotations: Vec::new() },
        ],
    }
}

fn written(archive: &ProjectArchive, images: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ArchiveWriter::new(Cursor::new(&mut bytes));
    let mut archive = archive.clone();
    for (hash, content) in images {
        let name = writer.add_image(hash, "png", &mut &content[..]).unwrap();
        archive.images.iter_mut().filter(|image| image.hash == *hash).for_each(|image| image.file = Some(name.clone()));
    }
    writer.finish(&archive).unwrap();
    bytes
}

#[test]
fn every_shape_tag_attribute_and_flag_survives_a_round_trip() {
    let original = project();
    let mut reader = ArchiveReader::open(Cursor::new(written(&original, &[]))).unwrap();
    assert_eq!(reader.project().unwrap(), original);
}

#[test]
fn included_images_are_read_back_by_name() {
    let original = project();
    let bytes = written(&original, &[("aa11", b"first image's bytes")]);
    let mut reader = ArchiveReader::open(Cursor::new(bytes)).unwrap();
    let project = reader.project().unwrap();
    assert_eq!(project.images[0].file.as_deref(), Some("images/aa11.png"));
    assert_eq!(project.images[1].file, None);
    let mut content = Vec::new();
    reader.image("images/aa11.png").unwrap().read_to_end(&mut content).unwrap();
    assert_eq!(content, b"first image's bytes");
    assert!(reader.image("images/../project.json").is_err(), "only images");
}

#[test]
fn the_format_version_is_written_and_newer_ones_are_refused() {
    let bytes = written(&project(), &[]);
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut json = String::new();
    zip.by_name("project.json").unwrap().read_to_string(&mut json).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!((&value["format"], &value["version"]), (&serde_json::json!("oxilabel-project"), &serde_json::json!(ARCHIVE_VERSION)));

    let newer = json.replace(&format!("\"version\":{ARCHIVE_VERSION}"), "\"version\":99");
    assert_ne!(newer, json);
    let error = ArchiveReader::open(Cursor::new(zip_with(&newer))).unwrap().project().unwrap_err();
    assert!(error.contains("newer") && error.contains("99"), "{error}");
}

#[test]
fn other_files_are_not_archives() {
    assert!(ArchiveReader::open(Cursor::new(b"not a zip".to_vec())).is_err());
    let error = ArchiveReader::open(Cursor::new(zip_with("{\"format\": \"something else\"}"))).unwrap().project().unwrap_err();
    assert!(error.contains("not an OxiLabel project archive"), "{error}");
}

fn zip_with(project_json: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
    zip.start_file("project.json", zip::write::SimpleFileOptions::default()).unwrap();
    std::io::Write::write_all(&mut zip, project_json.as_bytes()).unwrap();
    zip.finish().unwrap();
    bytes
}
