//! Export requests and results, shared by the server and the UI. The formats themselves live in
//! the `rtag-formats` crate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Whether an export includes the image files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageMode {
    /// Only labels and metadata.
    #[default]
    None,
    /// Symbolic links to the original files (no copying; usable on the same machine).
    Symlink,
    Copy,
}

/// The part of a dataset an image belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subset {
    Train,
    Val,
    Test,
}

impl Subset {
    pub const ALL: [Subset; 3] = [Subset::Train, Subset::Val, Subset::Test];

    pub fn name(self) -> &'static str {
        match self {
            Subset::Train => "train",
            Subset::Val => "val",
            Subset::Test => "test",
        }
    }
}

/// How images are divided into train, val and test, in percent (summing to 100).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub train: u8,
    pub val: u8,
    pub test: u8,
}

impl Default for Split {
    fn default() -> Self {
        Split { train: 80, val: 20, test: 0 }
    }
}

impl Split {
    pub fn validate(&self) -> Result<(), String> {
        let total = u16::from(self.train) + u16::from(self.val) + u16::from(self.test);
        if total != 100 {
            return Err(format!("train, val and test must add up to 100% (they add up to {total}%)"));
        }
        if self.train == 0 {
            return Err("the train subset cannot be empty".into());
        }
        Ok(())
    }
}

/// Which images of the project an export contains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSelection {
    All,
    /// Images with at least one annotation.
    WithAnnotations,
    Ids { ids: Vec<i64> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRequest {
    /// An exporter id, e.g. `yolo-detect`.
    pub format: String,
    pub images: ImageSelection,
    #[serde(default)]
    pub include_images: ImageMode,
    #[serde(default)]
    pub split: Split,
}

/// A finished export: a folder on the server, also downloadable as a zip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportResult {
    pub name: String,
    /// The export folder as the server sees it.
    pub path: String,
    pub image_count: usize,
    pub annotation_count: usize,
    pub subsets: BTreeMap<Subset, usize>,
    /// Where to download the export as a zip.
    pub download: String,
}

/// An available export format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFormat {
    pub id: String,
    pub name: String,
}
