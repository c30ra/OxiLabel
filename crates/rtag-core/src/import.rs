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
    /// Where to look for export images the project does not have yet (by file name, in any
    /// subfolder, at the same size); those found are added to the project. A path relative to
    /// the image root, or absolute from the server's own machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images_folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvatImportReport {
    pub labels: Vec<CvatLabelReport>,
    /// Images in the export.
    pub images: usize,
    /// Images matched to one of the project's by file name and size, or found in the images
    /// folder.
    pub matched: usize,
    /// Of those, the ones found in the images folder, added to the project (or that would be).
    pub added: usize,
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

/// How to import a project archive: the new project's name (the archive's if none), and where
/// to look for images the archive only refers to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchiveImportPlan {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub images_folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveImportReport {
    pub project: crate::Project,
    /// Images added to the new project.
    pub images: usize,
    /// Archive images found neither in it, at their path, nor in the images folder (their
    /// annotations are not imported).
    pub missing: Vec<String>,
    pub annotations: usize,
    /// Annotations that do not fit their image or label.
    pub skipped: Vec<SkipCount>,
    /// Numbers of the jobs whose assignee has no account here.
    pub unassigned_jobs: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipCount {
    pub reason: String,
    pub count: usize,
}
