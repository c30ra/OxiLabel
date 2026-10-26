# 03: Project read model for exporters and project archives

**What to build:** Exports and project archive downloads read a project through one module: the project, its labels, and its images in path order, each with its annotations and its resolved source file, with a choice of including pending pre-annotations or not. Exporters map it to their export input; project archives map it to the archive. The column lists for label and annotation rows are owned once, beside the code that turns rows into labels and annotations, so adding a label or annotation field is one change. Image source files are resolved the same way for both (the archive's canonical, never-outside-the-root rule).

**Blocked by:** 01

**Status:** done

- [x] Exports and project archives no longer contain their own project, label, image or annotation queries
- [x] Label and annotation column lists appear once each
- [x] An export and an archive of the same project resolve every image to the same file
- [x] Read model tests on in-memory SQLite cover image order, annotation order within an image, the pending filter, and images without annotations
- [x] Existing export and archive HTTP tests and the exporter tests still pass

## Comments

- Implemented in `crates/oxilabel-server/src/project_read_model.rs`: `read(conn, root, project, Pending::{Include, Exclude}) -> ProjectRead` (name, label/job counters, labels, images in path order with id, path, `source`, hash, size, annotated, job number, annotations in drawing order). `source` is the image path under the canonical image root (`images::image_root`), or the absolute path as it is; exports used the non-canonical `state.images_root` before, now both use the archive's rule. Exports (`Pending::Exclude`) filter the image selection and map to `ExportImage`; archives (`Pending::Include`) map to `ArchiveImage` and copy files from `source`. The archive's job list query stays in `archives.rs` (jobs are not part of the read model).
- Column lists: `ANNOTATION_COLUMNS` + `bind_annotation` beside `annotation_from_row` in `annotation_store.rs` (store load/insert and the read model use it); `LABEL_FIELDS` + `bind_label` beside `label_from_row` in `labels.rs` (select, insert, and the update as one row-value `SET (…) = (…)`). Copying labels into a new project now reads and stores them through `labels` instead of its own `INSERT … SELECT`. `placeholders(columns)` in `lib.rs` builds the `?, ?, …` lists.
