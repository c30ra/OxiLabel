//! Importers and exporters. An exporter turns a project's labels and annotated images into files;
//! it writes through an [`ExportSink`], which decides where the files go (a folder, memory in
//! tests) and whether images are included. Adding a format means implementing [`Exporter`] and
//! listing it in [`exporters`]; the server and the core need no changes.

mod yolo;

use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}};

pub use rtag_core::export::{ImageMode, Split, Subset};
use rtag_core::{Label, annotation::Annotation};
use serde::{Deserialize, Serialize};

pub use yolo::YoloDetect;

/// Every available exporter.
pub fn exporters() -> Vec<Box<dyn Exporter>> {
    vec![Box::new(YoloDetect)]
}

pub fn exporter(id: &str) -> Option<Box<dyn Exporter>> {
    exporters().into_iter().find(|exporter| exporter.id() == id)
}

pub trait Exporter: Send + Sync {
    /// Stable identifier used in requests, e.g. `yolo-detect`.
    fn id(&self) -> &'static str;
    /// Name shown to people.
    fn name(&self) -> &'static str;
    /// Writes the export. The manifest is written by [`run_export`], not by the exporter.
    fn export(&self, input: &ExportInput, sink: &mut dyn ExportSink) -> io::Result<()>;
}

/// One image of the export with its annotations in original-image coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportImage {
    /// Path relative to the image root, with `/` separators; unique within the project.
    pub path: String,
    /// Where the server reads the image file, for sinks that include images.
    pub source: PathBuf,
    /// The image's content hash; decides its subset.
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub annotations: Vec<Annotation>,
}

impl ExportImage {
    /// A file name unique within the export: the relative path with folders joined by `__`.
    pub fn flat_name(&self) -> String {
        self.path.replace('/', "__")
    }

    /// `flat_name` without its extension.
    pub fn flat_stem(&self) -> String {
        let name = self.flat_name();
        match name.rfind('.') {
            Some(dot) if dot > 0 => name[..dot].to_owned(),
            _ => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportInput {
    pub project: String,
    pub labels: Vec<Label>,
    pub images: Vec<ExportImage>,
    pub split: Split,
}

impl ExportInput {
    pub fn subset(&self, image: &ExportImage) -> Subset {
        subset_for(&self.split, &image.hash)
    }
}

/// The subset an image with content hash `hash` belongs to. It comes from the hash, not from
/// chance: the same image always lands in the same subset for the same percentages, and adding
/// images never moves existing ones.
pub fn subset_for(split: &Split, hash: &str) -> Subset {
    // A stable number from 0 to 9999 per image.
    let digest = blake3::hash(hash.as_bytes());
    let bytes: [u8; 8] = digest.as_bytes()[..8].try_into().expect("8 bytes");
    let slot = u64::from_le_bytes(bytes) % 10_000;
    let train = u64::from(split.train) * 100;
    let val = train + u64::from(split.val) * 100;
    if slot < train {
        Subset::Train
    } else if slot < val {
        Subset::Val
    } else {
        Subset::Test
    }
}

/// Where an exporter's files go.
pub trait ExportSink {
    /// Writes a file at `path` (relative, `/` separators) with `contents`.
    fn write(&mut self, path: &str, contents: &[u8]) -> io::Result<()>;
    /// Places the image file `source` at `path`, or nothing, depending on the sink's image mode.
    fn image(&mut self, path: &str, source: &Path) -> io::Result<()>;
}

/// Writes into a folder on disk.
pub struct FolderSink {
    root: PathBuf,
    images: ImageMode,
}

impl FolderSink {
    pub fn new(root: impl Into<PathBuf>, images: ImageMode) -> Self {
        FolderSink { root: root.into(), images }
    }

    fn target(&self, path: &str) -> io::Result<PathBuf> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(target)
    }
}

impl ExportSink for FolderSink {
    fn write(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        fs::write(self.target(path)?, contents)
    }

    fn image(&mut self, path: &str, source: &Path) -> io::Result<()> {
        match self.images {
            ImageMode::None => Ok(()),
            ImageMode::Copy => fs::copy(source, self.target(path)?).map(|_| ()),
            #[cfg(unix)]
            ImageMode::Symlink => std::os::unix::fs::symlink(source, self.target(path)?),
            #[cfg(not(unix))]
            ImageMode::Symlink => fs::copy(source, self.target(path)?).map(|_| ()),
        }
    }
}

/// Keeps written files in memory; images are recorded by source path. For tests.
#[derive(Debug, Default)]
pub struct MemorySink {
    pub files: BTreeMap<String, Vec<u8>>,
    pub images: BTreeMap<String, PathBuf>,
}

impl ExportSink for MemorySink {
    fn write(&mut self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.files.insert(path.to_owned(), contents.to_vec());
        Ok(())
    }

    fn image(&mut self, path: &str, source: &Path) -> io::Result<()> {
        self.images.insert(path.to_owned(), source.to_owned());
        Ok(())
    }
}

/// What an export contained, as written to `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    /// When the export was made (UTC, RFC 3339).
    pub created: String,
    pub project: String,
    pub labels: Vec<ManifestLabel>,
    pub image_count: usize,
    pub annotation_count: usize,
    /// blake3 of the exported images' paths and annotations: equal hashes mean equal data.
    pub annotation_hash: String,
    pub images: ImageMode,
    pub split: Split,
    /// Number of images in each subset.
    pub subsets: BTreeMap<Subset, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestLabel {
    pub id: i64,
    pub name: String,
    pub ignore: bool,
}

/// Runs `exporter` and writes `manifest.json` next to its output.
pub fn run_export(
    exporter: &dyn Exporter,
    input: &ExportInput,
    sink: &mut dyn ExportSink,
    created: &str,
    images: ImageMode,
) -> io::Result<Manifest> {
    exporter.export(input, sink)?;
    let manifest = Manifest {
        format: exporter.id().to_owned(),
        created: created.to_owned(),
        project: input.project.clone(),
        labels: input
            .labels
            .iter()
            .map(|label| ManifestLabel { id: label.id, name: label.name.clone(), ignore: label.ignore })
            .collect(),
        image_count: input.images.len(),
        annotation_count: input.images.iter().map(|image| image.annotations.len()).sum(),
        annotation_hash: annotation_hash(&input.images),
        images,
        split: input.split,
        subsets: Subset::ALL
            .iter()
            .map(|&subset| (subset, input.images.iter().filter(|image| input.subset(image) == subset).count()))
            .collect(),
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
    sink.write("manifest.json", &json)?;
    Ok(manifest)
}

/// Hash over the images (sorted by path) and their annotations in order.
pub fn annotation_hash(images: &[ExportImage]) -> String {
    let mut sorted: Vec<&ExportImage> = images.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hasher = blake3::Hasher::new();
    for image in sorted {
        hasher.update(image.path.as_bytes());
        hasher.update(&[0]);
        hasher.update(&serde_json::to_vec(&image.annotations).expect("annotations serialize"));
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex().to_string()
}
