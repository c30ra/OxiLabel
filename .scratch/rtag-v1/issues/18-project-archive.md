# 18: Project archive

**What to build:** I can export a whole Project as a Project archive (versioned zip with labels, attributes, jobs, annotations in original coordinates, image references with hashes, and optionally the images) and import it into another rtag instance without loss.

**Blocked by:** 08, 09, 10

**Status:** done

- [x] Exporting produces an archive with a format version and the option to include images
- [x] Importing an archive recreates the project, labels (with the same permanent ids), jobs and annotations
- [x] Export followed by import is lossless for all supported shapes, tags and attributes
- [x] Importing an archive from a newer unsupported version fails with a clear message
- [x] Formats tests cover round-trips for every shape type

**Decisions (2026-10-18):** an archive without images refers to each by path and content hash; on import an image is taken from its old path if the file there has the same hash, else looked for by file name in an images folder chosen in the import dialog (checked by hash), else listed as missing and left out with its annotations. Jobs go to the user of the same name if there is one, else stay unassigned and are listed. An archive always becomes a new project (named as in the archive unless renamed in the dialog).

**Implementation note:** `rtag-formats/src/archive.rs`: a zip with `project.json` (`"format": "rtag-project"`, `"version": 1`; the project's name and next label id and job number, labels as stored, jobs by number with the assignee's username, images with path, hash, size, status, job number, annotations as rtag's JSON including source, confidence, pending, hidden and locked) and included images under `images/<hash>.<ext>`, stored uncompressed. `ArchiveReader::project` refuses other files and newer versions with the version in the message. Server (`rtag-server/src/archives.rs`, admins): `GET /api/projects/{p}/archive?images=true|false` writes the zip to an unnamed temp file in the export folder and streams it; `POST /api/project-archives` (multipart `file` + `plan` {name, images_folder}, no size limit, the upload streamed to disk) reads and locates images before one transaction that creates the project, labels with their ids, jobs, images (included copies kept under the upload folder, ADR 0003) and annotations (checked against their label and image). Absolute image paths are only used for requests from the server's machine. UI: "Archive…" on the project page (refer to images or include them, then Download), "Import archive…" on the projects page (file, name, images folder with Browse… on the server's machine); a clean import opens the new project, otherwise a report lists missing images, unassigned jobs and skipped annotations. After review: copies of one image share one file in an archive; two archived images found as one file are imported once (the second listed as missing); an import that fails removes the copies it took out; a blank name becomes "Imported project"; `project.json` and each included image are read up to a size limit; the next label id and job number never fall below ids in use; downloads carry an ASCII file name and the UTF-8 one (RFC 5987); the projects list refreshes after an import.
