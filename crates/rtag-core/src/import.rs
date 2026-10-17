//! Importing a CVAT export into a project: what to do with its labels and with annotations
//! already on the images (the plan), and what the import finds or did (the report). The same
//! request previews the import and, with `commit`, carries it out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What becomes of the annotations of one CVAT label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LabelChoice {
    /// They get this existing label of the project.
    Map { label: i64 },
    /// A new label is made from the CVAT one (name, color, shapes, attributes).
    Create,
    /// They are left out.
    Skip,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CvatImportPlan {
    /// By CVAT label name; a label not listed gets the report's suggested choice.
    #[serde(default)]
    pub labels: BTreeMap<String, LabelChoice>,
    /// Replace the annotations of matched images instead of adding to them.
    #[serde(default)]
    pub replace: bool,
    /// Write the import; otherwise only report what it would do.
    #[serde(default)]
    pub commit: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvatImportReport {
    pub labels: Vec<CvatLabelReport>,
    /// Images in the export.
    pub images: usize,
    /// Images matched to one of the project's by file name and size.
    pub matched: usize,
    /// Export images with no project image of that file name.
    pub unmatched: Vec<String>,
    /// Export images whose file name several project images have.
    pub ambiguous: Vec<String>,
    /// Export images whose project image has another size, e.g. a resized copy.
    pub size_mismatch: Vec<String>,
    /// Matched images that already have annotations.
    pub annotated: usize,
    /// Annotations imported (or that would be).
    pub annotations: usize,
    /// Shapes left out, with why and how many.
    pub skipped: Vec<SkipCount>,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvatLabelReport {
    pub name: String,
    pub color: Option<String>,
    /// Shapes and tags of this label in the export.
    pub annotations: usize,
    /// The plan's choice, or the suggestion: the project label of the same name, else a new one.
    pub choice: LabelChoice,
    /// Why the label cannot be created as it is, if it cannot.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipCount {
    pub reason: String,
    pub count: usize,
}
