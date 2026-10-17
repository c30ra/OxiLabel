//! YOLO (Ultralytics) datasets: `images/<subset>/<image>` and `labels/<subset>/<image>.txt`, and
//! `data.yaml` naming the subsets and classes. Each label file has one line per object, its
//! coordinates normalised to the image size:
//!
//! - detect: `class cx cy w h`, every shape as its bounding box;
//! - segment: `class x1 y1 x2 y2 …`, the polygon of each area the shape covers (see `regions`);
//! - OBB: `class x1 y1 … x4 y4`, the corners of a box: exact for (turned) rectangles, otherwise
//!   the tightest turned box around each area.
//!
//! Class numbers are the permanent label ids minus one, so they never shift: a deleted label
//! leaves a placeholder name in `data.yaml`. Shapes of the ignore label are left out, as YOLO has
//! no ignore regions; tags are left out too.

use std::{collections::BTreeMap, fmt::Write as _, io};

use rtag_core::{
    Label,
    annotation::{Annotation, Shape},
    geometry::{Point, min_area_rect},
};

use crate::{
    ExportImage, ExportInput, ExportSink, Exporter, ImageMode, Subset,
    regions::{Detail, regions},
};

pub struct YoloDetect;
pub struct YoloSegment;
pub struct YoloObb;

/// Ultralytics finds each label file by replacing `/images/` with `/labels/` in the image path,
/// so the images must be in the export next to the labels.
const IMAGE_MODES: &[ImageMode] = &[ImageMode::Symlink, ImageMode::Copy];

impl Exporter for YoloDetect {
    fn id(&self) -> &'static str {
        "yolo-detect"
    }

    fn name(&self) -> &'static str {
        "YOLO detect"
    }

    fn image_modes(&self) -> &'static [ImageMode] {
        IMAGE_MODES
    }

    fn export(&self, input: &ExportInput, sink: &mut dyn ExportSink) -> io::Result<()> {
        export(input, sink, "detect", &|annotation, _, image| {
            let bounds = annotation.shape.bounds();
            let (width, height) = (f64::from(image.width), f64::from(image.height));
            let (cx, cy) = ((bounds.min.x + bounds.max.x) / 2.0 / width, (bounds.min.y + bounds.max.y) / 2.0 / height);
            vec![vec![cx, cy, bounds.width() / width, bounds.height() / height]]
        })
    }
}

impl Exporter for YoloSegment {
    fn id(&self) -> &'static str {
        "yolo-segment"
    }

    fn name(&self) -> &'static str {
        "YOLO segment"
    }

    fn image_modes(&self) -> &'static [ImageMode] {
        IMAGE_MODES
    }

    fn export(&self, input: &ExportInput, sink: &mut dyn ExportSink) -> io::Result<()> {
        export(input, sink, "segment", &|annotation, label, image| {
            regions(annotation, label, (image.width, image.height), Detail::Simplified).iter().map(|polygon| normalised(polygon, image)).collect()
        })
    }
}

impl Exporter for YoloObb {
    fn id(&self) -> &'static str {
        "yolo-obb"
    }

    fn name(&self) -> &'static str {
        "YOLO OBB"
    }

    fn image_modes(&self) -> &'static [ImageMode] {
        IMAGE_MODES
    }

    fn export(&self, input: &ExportInput, sink: &mut dyn ExportSink) -> io::Result<()> {
        export(input, sink, "OBB", &|annotation, label, image| match &annotation.shape {
            Shape::Rectangle(_) | Shape::OrientedRectangle(_) => vec![normalised(&annotation.shape.vertices(), image)],
            // The box around the ellipse along its axes, not the box of its polygon (which lies
            // inside it).
            Shape::Ellipse(ellipse) => vec![normalised(&ellipse.bounding_box().corners(), image)],
            // Every corner, so the box encloses every pixel.
            _ => regions(annotation, label, (image.width, image.height), Detail::Exact)
                .iter()
                .map(|polygon| normalised(&min_area_rect(polygon), image))
                .collect(),
        })
    }
}

/// The values of each object of an annotation: coordinates normalised to the image.
type Objects<'a> = &'a dyn Fn(&Annotation, &Label, &ExportImage) -> Vec<Vec<f64>>;

/// Writes a YOLO dataset of `kind` (for `data.yaml`): a label file per image with a line per
/// object `objects` gives, and the images next to them.
fn export(input: &ExportInput, sink: &mut dyn ExportSink, kind: &str, objects: Objects) -> io::Result<()> {
    let labels: BTreeMap<i64, &Label> = input.labels.iter().map(|label| (label.id, label)).collect();
    for image in &input.images {
        let mut lines = String::new();
        for annotation in &image.annotations {
            // Tags have no area; ignore-label regions are not training targets.
            let Some(label) = labels.get(&annotation.label).filter(|label| !label.ignore && !annotation.shape.is_tag()) else { continue };
            for values in objects(annotation, label, image) {
                write!(lines, "{}", annotation.label - 1).expect("writing to a string");
                for value in values {
                    write!(lines, " {value:.6}").expect("writing to a string");
                }
                lines.push('\n');
            }
        }
        // An image without objects still gets a (empty) label file: YOLO treats it as background.
        let subset = input.subset(image).name();
        sink.write(&format!("labels/{subset}/{}.txt", image.flat_stem()), lines.as_bytes())?;
        sink.image(&format!("images/{subset}/{}", image.flat_name()), &image.source)?;
    }
    sink.write("data.yaml", data_yaml(input, kind).as_bytes())
}

/// `points` as normalised coordinates, kept inside the image.
fn normalised(points: &[Point], image: &ExportImage) -> Vec<f64> {
    let (width, height) = (f64::from(image.width), f64::from(image.height));
    points.iter().flat_map(|p| [(p.x / width).clamp(0.0, 1.0), (p.y / height).clamp(0.0, 1.0)]).collect()
}

fn data_yaml(input: &ExportInput, kind: &str) -> String {
    let names: BTreeMap<i64, &str> = input.labels.iter().map(|label| (label.id, label.name.as_str())).collect();
    let highest = names.keys().max().copied().unwrap_or(0);
    let mut yaml = format!(
        "# YOLO {kind} dataset exported by rtag from project \"{}\".\n\
         # Class numbers are label ids minus one; unused numbers are placeholders.\n\
         # Paths are relative to this file's folder.\n\
         train: images/train\n",
        input.project.replace('"', "'")
    );
    // Ultralytics needs a validation set; without one it validates on the training images.
    let used = |subset| input.images.iter().any(|image| input.subset(image) == subset);
    if used(Subset::Val) {
        yaml.push_str("val: images/val\n");
    } else {
        yaml.push_str("val: images/train  # no images in the val subset\n");
    }
    if used(Subset::Test) {
        yaml.push_str("test: images/test\n");
    }
    yaml.push_str("names:\n");
    for id in 1..=highest {
        let name = names.get(&id).map_or_else(|| format!("unused_{id}"), |name| (*name).to_owned());
        writeln!(yaml, "  {}: {}", id - 1, yaml_string(&name)).expect("writing to a string");
    }
    yaml
}

/// Quotes a YAML scalar when it would otherwise be misread.
fn yaml_string(value: &str) -> String {
    let plain = !value.is_empty()
        && value.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        && !value.starts_with(' ')
        && !value.ends_with(' ')
        && !matches!(value.to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no" | "null" | "on" | "off")
        && value.parse::<f64>().is_err();
    if plain { value.to_owned() } else { format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")) }
}
