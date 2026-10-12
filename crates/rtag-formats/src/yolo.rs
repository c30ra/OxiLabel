//! YOLO detect (Ultralytics): `images/<subset>/<image>` and `labels/<subset>/<image>.txt` with one
//! `class cx cy w h` line per box, normalised to the image size, and `data.yaml` naming the
//! subsets and classes.
//!
//! Class numbers are the permanent label ids minus one, so they never shift: a deleted label
//! leaves a placeholder name in `data.yaml`. Boxes of the ignore label are left out, as detection
//! has no ignore regions. Every shape is exported as its bounding box; tags are left out.

use std::{collections::BTreeMap, fmt::Write as _, io};

use crate::{ExportInput, ExportSink, Exporter, ImageMode, Subset};

pub struct YoloDetect;

impl Exporter for YoloDetect {
    fn id(&self) -> &'static str {
        "yolo-detect"
    }

    fn name(&self) -> &'static str {
        "YOLO detect"
    }

    /// Ultralytics finds each label file by replacing `/images/` with `/labels/` in the image path,
    /// so the images must be in the export next to the labels.
    fn image_modes(&self) -> &'static [ImageMode] {
        &[ImageMode::Symlink, ImageMode::Copy]
    }

    fn export(&self, input: &ExportInput, sink: &mut dyn ExportSink) -> io::Result<()> {
        let ignored: Vec<i64> = input.labels.iter().filter(|label| label.ignore).map(|label| label.id).collect();
        for image in &input.images {
            let (width, height) = (f64::from(image.width), f64::from(image.height));
            let mut lines = String::new();
            // Tags have no box; ignore-label regions are not training targets.
            let boxes = image.annotations.iter().filter(|annotation| !annotation.shape.is_tag() && !ignored.contains(&annotation.label));
            for annotation in boxes {
                let bounds = annotation.shape.bounds();
                let class = annotation.label - 1;
                let (cx, cy) = ((bounds.min.x + bounds.max.x) / 2.0 / width, (bounds.min.y + bounds.max.y) / 2.0 / height);
                let (w, h) = (bounds.width() / width, bounds.height() / height);
                writeln!(lines, "{class} {cx:.6} {cy:.6} {w:.6} {h:.6}").expect("writing to a string");
            }
            // An image without boxes still gets a (empty) label file: YOLO treats it as background.
            let subset = input.subset(image).name();
            sink.write(&format!("labels/{subset}/{}.txt", image.flat_stem()), lines.as_bytes())?;
            sink.image(&format!("images/{subset}/{}", image.flat_name()), &image.source)?;
        }
        sink.write("data.yaml", data_yaml(input).as_bytes())
    }
}

fn data_yaml(input: &ExportInput) -> String {
    let names: BTreeMap<i64, &str> = input.labels.iter().map(|label| (label.id, label.name.as_str())).collect();
    let highest = names.keys().max().copied().unwrap_or(0);
    let mut yaml = format!(
        "# YOLO detect dataset exported by rtag from project \"{}\".\n\
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
