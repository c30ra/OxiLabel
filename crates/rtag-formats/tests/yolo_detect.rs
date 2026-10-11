use std::{collections::BTreeMap, fs, path::Path};

use rtag_core::{
    Label, ShapeType,
    annotation::{Annotation, Shape},
    geometry::Rect,
};
use rtag_formats::{
    ExportImage, ExportInput, ImageMode, MemorySink, Split, Subset, YoloDetect, exporter, exporters, run_export,
};

fn label(id: i64, name: &str, ignore: bool) -> Label {
    Label {
        id,
        name: name.into(),
        color: "#ffffff".into(),
        hotkey: None,
        shape_types: vec![ShapeType::Rectangle],
        ignore,
    }
}

fn boxed(label: i64, min: (f64, f64), max: (f64, f64)) -> Annotation {
    Annotation::manual(label, Shape::Rectangle(Rect::new(min.0, min.1, max.0, max.1)))
}

fn image(path: &str, hash: &str, annotations: Vec<Annotation>) -> ExportImage {
    ExportImage {
        path: path.into(),
        source: Path::new("/images").join(path),
        hash: hash.into(),
        width: 200,
        height: 100,
        annotations,
    }
}

/// Labels 1 and 2, label 3 deleted, label 4 the ignore label; one image with boxes, one without.
fn cells(split: Split) -> ExportInput {
    ExportInput {
        project: "Cells".into(),
        labels: vec![label(1, "crack", false), label(2, "finger", false), label(4, "unsure: maybe", true)],
        images: vec![
            image(
                "set/a.png",
                "hash-a",
                vec![boxed(1, (20.0, 10.0), (60.0, 50.0)), boxed(2, (100.0, 0.0), (200.0, 100.0)), boxed(4, (0.0, 0.0), (10.0, 10.0))],
            ),
            image("other/b.png", "hash-b", vec![]),
        ],
        split,
    }
}

/// Every file under `dir`, keyed by its path relative to `dir` with `/` separators.
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                files.insert(relative, fs::read_to_string(&path).unwrap());
            }
        }
    }
    files
}

const ALL_TRAIN: Split = Split { train: 100, val: 0, test: 0 };

#[test]
fn yolo_detect_matches_the_fixture() {
    let mut sink = MemorySink::default();
    run_export(&YoloDetect, &cells(ALL_TRAIN), &mut sink, "2026-10-11T12:00:00Z", ImageMode::None).unwrap();
    let mut written: BTreeMap<String, String> =
        sink.files.iter().map(|(path, bytes)| (path.clone(), String::from_utf8(bytes.clone()).unwrap())).collect();
    written.remove("manifest.json").expect("a manifest is written");
    let expected = read_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yolo-detect"));
    assert_eq!(written, expected);
}

#[test]
fn images_go_next_to_their_label_files_in_the_same_subset() {
    let mut sink = MemorySink::default();
    run_export(&YoloDetect, &cells(ALL_TRAIN), &mut sink, "now", ImageMode::Symlink).unwrap();
    assert_eq!(sink.images.get("images/train/set__a.png").unwrap(), Path::new("/images/set/a.png"));
    assert_eq!(sink.images.len(), 2);
}

#[test]
fn the_manifest_records_what_was_exported() {
    let mut sink = MemorySink::default();
    let manifest = run_export(&YoloDetect, &cells(ALL_TRAIN), &mut sink, "2026-10-11T12:00:00Z", ImageMode::Copy).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&sink.files["manifest.json"]).unwrap();
    assert_eq!(json["format"], "yolo-detect");
    assert_eq!(json["created"], "2026-10-11T12:00:00Z");
    assert_eq!(json["project"], "Cells");
    assert_eq!(json["image_count"], 2);
    assert_eq!(json["annotation_count"], 3);
    assert_eq!(json["images"], "copy");
    assert_eq!(json["split"], serde_json::json!({"train": 100, "val": 0, "test": 0}));
    assert_eq!(json["subsets"], serde_json::json!({"train": 2, "val": 0, "test": 0}));
    assert_eq!(json["labels"][2], serde_json::json!({"id": 4, "name": "unsure: maybe", "ignore": true}));
    assert_eq!(json["annotation_hash"], manifest.annotation_hash);
}

#[test]
fn the_annotation_hash_changes_with_the_annotations_but_not_the_image_order() {
    let hash = |input: &ExportInput| rtag_formats::annotation_hash(&input.images);
    let base = cells(ALL_TRAIN);
    let mut reordered = base.clone();
    reordered.images.reverse();
    assert_eq!(hash(&base), hash(&reordered));
    let mut changed = base.clone();
    changed.images[0].annotations[0] = boxed(1, (21.0, 10.0), (60.0, 50.0));
    assert_ne!(hash(&base), hash(&changed));
}

#[test]
fn the_split_is_stable_and_close_to_the_percentages() {
    let split = Split { train: 70, val: 20, test: 10 };
    let subsets: Vec<Subset> = (0..2000).map(|i| rtag_formats::subset_for(&split, &format!("hash-{i}"))).collect();
    let share = |subset| subsets.iter().filter(|s| **s == subset).count() as f64 / 2000.0;
    assert!((share(Subset::Train) - 0.7).abs() < 0.04, "train {}", share(Subset::Train));
    assert!((share(Subset::Val) - 0.2).abs() < 0.04, "val {}", share(Subset::Val));
    assert!((share(Subset::Test) - 0.1).abs() < 0.04, "test {}", share(Subset::Test));
    // The same image always lands in the same subset.
    assert_eq!(rtag_formats::subset_for(&split, "hash-7"), subsets[7]);
}

#[test]
fn split_subsets_get_their_own_folders_and_yaml_entries() {
    let split = Split { train: 50, val: 30, test: 20 };
    let images: Vec<ExportImage> = (0..40).map(|i| image(&format!("set/{i}.png"), &format!("hash-{i}"), vec![])).collect();
    let input = ExportInput { images, ..cells(split) };
    let mut sink = MemorySink::default();
    run_export(&YoloDetect, &input, &mut sink, "now", ImageMode::None).unwrap();
    let yaml = String::from_utf8(sink.files["data.yaml"].clone()).unwrap();
    assert!(yaml.contains("train: images/train\nval: images/val\ntest: images/test\n"), "{yaml}");
    for subset in Subset::ALL {
        let expected = input.images.iter().filter(|image| input.subset(image) == subset).count();
        let written = sink.files.keys().filter(|path| path.starts_with(&format!("labels/{}/", subset.name()))).count();
        assert_eq!(written, expected, "{subset:?}");
        assert!(expected > 0, "{subset:?} is empty with 40 images");
    }
}

#[test]
fn split_percentages_must_add_up() {
    assert_eq!(Split { train: 80, val: 20, test: 0 }.validate(), Ok(()));
    assert!(Split { train: 80, val: 30, test: 0 }.validate().is_err());
    assert!(Split { train: 0, val: 100, test: 0 }.validate().is_err());
}

#[test]
fn exporters_are_found_by_id() {
    assert_eq!(exporter("yolo-detect").unwrap().name(), "YOLO detect");
    assert!(exporter("nope").is_none());
    assert_eq!(exporters().len(), 1);
}
