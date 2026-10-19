//! Project archives: OxiLabel's own lossless format for moving a whole project between OxiLabel
//! instances. A zip with `project.json` (the project, its labels with their permanent ids, jobs,
//! and per image its reference, content hash, status, job and annotations in original image
//! coordinates) and, optionally, the image files under `images/`, named by content hash.
//!
//! `project.json` carries `"format": "oxilabel-project"` and a `version`; a reader refuses versions
//! newer than `ARCHIVE_VERSION`, and keeps reading older ones.

use std::io::{self, Read, Seek, Write};

use oxilabel_core::{Label, annotation::Annotation};
use serde::{Deserialize, Serialize};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The newest archive format this OxiLabel writes and reads.
pub const ARCHIVE_VERSION: u32 = 1;
const FORMAT: &str = "oxilabel-project";
const PROJECT_FILE: &str = "project.json";
const IMAGES_FOLDER: &str = "images/";
/// The most of `project.json` read: far beyond any real project, short of exhausting memory.
const PROJECT_LIMIT: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectArchive {
    pub name: String,
    /// The next label id and job number the project would hand out, so deleted ones stay unused.
    pub next_label_id: i64,
    pub next_job_number: i64,
    pub labels: Vec<Label>,
    pub jobs: Vec<ArchiveJob>,
    pub images: Vec<ArchiveImage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveJob {
    pub number: i64,
    /// The assignee's username.
    pub assignee: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveImage {
    /// As the project stored it: relative to the image root, or absolute.
    pub path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub annotated: bool,
    /// The number of its job.
    pub job: Option<i64>,
    /// Where the archive holds the file, if it includes it.
    pub file: Option<String>,
    pub annotations: Vec<Annotation>,
}

/// `project.json` as written: the format, its version, then the project.
#[derive(Serialize)]
struct Written<'a> {
    format: &'static str,
    version: u32,
    #[serde(flatten)]
    project: &'a ProjectArchive,
}

#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<u32>,
}

pub struct ArchiveWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
}

impl<W: Write + Seek> ArchiveWriter<W> {
    pub fn new(out: W) -> Self {
        ArchiveWriter { zip: ZipWriter::new(out) }
    }

    /// Adds an image file (`extension` without the dot) and returns its name in the archive, for
    /// `ArchiveImage::file`. Images are stored as they are: they are compressed already.
    pub fn add_image(&mut self, hash: &str, extension: &str, file: &mut dyn Read) -> io::Result<String> {
        let name = format!("{IMAGES_FOLDER}{hash}.{}", extension.to_ascii_lowercase());
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
        self.zip.start_file(name.as_str(), options)?;
        io::copy(file, &mut self.zip)?;
        Ok(name)
    }

    /// Writes `project.json` and closes the archive.
    pub fn finish(mut self, project: &ProjectArchive) -> io::Result<W> {
        self.zip.start_file(PROJECT_FILE, SimpleFileOptions::default())?;
        serde_json::to_writer(&mut self.zip, &Written { format: FORMAT, version: ARCHIVE_VERSION, project })?;
        Ok(self.zip.finish()?)
    }
}

pub struct ArchiveReader<R: Read + Seek> {
    zip: ZipArchive<R>,
}

impl<R: Read + Seek> ArchiveReader<R> {
    pub fn open(input: R) -> Result<Self, String> {
        ZipArchive::new(input).map(|zip| ArchiveReader { zip }).map_err(|err| format!("not an OxiLabel project archive (unreadable zip: {err})"))
    }

    /// The project, unless this is no archive or one of a newer format.
    pub fn project(&mut self) -> Result<ProjectArchive, String> {
        let not_archive = || "not an OxiLabel project archive (no OxiLabel project.json in it)".to_owned();
        let mut json = String::new();
        let entry = self.zip.by_name(PROJECT_FILE).map_err(|_| not_archive())?;
        entry.take(PROJECT_LIMIT).read_to_string(&mut json).map_err(|err| format!("unreadable project.json: {err}"))?;
        let header: Header = serde_json::from_str(&json).map_err(|_| not_archive())?;
        if header.format.as_deref() != Some(FORMAT) {
            return Err(not_archive());
        }
        match header.version {
            Some(version) if version > ARCHIVE_VERSION => Err(format!(
                "this archive was made by a newer OxiLabel (format version {version}); this one reads up to version {ARCHIVE_VERSION}: update OxiLabel to import it"
            )),
            Some(_) => serde_json::from_str(&json).map_err(|err| format!("the archive's project.json is damaged: {err}")),
            None => Err(not_archive()),
        }
    }

    /// An included image file, by its `ArchiveImage::file` name.
    pub fn image(&mut self, name: &str) -> Result<impl Read + '_, String> {
        let plain = name.strip_prefix(IMAGES_FOLDER).is_some_and(|rest| !rest.is_empty() && !rest.contains('/') && !rest.starts_with('.'));
        if !plain {
            return Err(format!("\"{name}\" is not an image of the archive"));
        }
        self.zip.by_name(name).map_err(|err| format!("the archive lacks {name}: {err}"))
    }
}
