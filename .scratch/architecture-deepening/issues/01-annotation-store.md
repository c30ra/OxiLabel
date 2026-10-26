# 01: Annotation store, with editor saves and pre-annotation runs as its first callers

**What to build:** One server module owns every change to an image's set of annotations, so the rules for that change live in one place. Its first two callers are the editor's save (replace the whole set, only if the version still matches) and a pre-annotation run (replace one model's pending pre-annotations on an image). The store checks each annotation against its label and the image size, allows one copy of each tag per image, assigns positions, and bumps the image version so a stale editor reloads. Only the editor save sets the image's annotated status from `counts_as_annotated`; ticket 26 (oxilabel-v1) decided that imports, pre-annotation runs and label deletes leave the status as it is, and the store keeps that rule. The store runs inside the caller's transaction.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] Editor saves and pre-annotation runs write annotations only through the store; their own INSERT, position and version SQL is gone
- [x] The shallow insert helper the writers called is no longer used by them
- [x] A pre-annotation run can no longer add a second copy of a tag the image already has (it is reported as skipped)
- [x] Store tests on in-memory SQLite cover each operation against each rule: per-annotation check, one tag per label, positions, version bump, annotated status only on save, stale version refused
- [x] Existing annotation and pre-annotation HTTP tests still pass

## Comments

Implemented in `crates/oxilabel-server/src/annotation_store.rs`, which runs on the caller's `&mut SqliteConnection` (its transaction). Operations: `load` (the image's annotations, version and status), `save` (editor: strict, version-guarded, sets `annotated`) and `write(conn, project, image, Replacing, annotations) -> Option<Written { stored, skipped }>` (lenient: refused annotations and repeated tags are skipped by reason, placed on top of the kept ones, version bumped, status untouched; `None` if the image is not in the project). `Replacing::{All, Pending { source }}`; ticket 02 adds an append variant for CVAT import. `annotations.rs` is now only the HTTP handlers; `inference.rs`'s `replace_pending` calls `write` with `Replacing::Pending`, and the run no longer checks shapes itself (it merges the store's skips). `check_annotation`, `insert_annotations` and `annotation_from_row` moved into the store; CVAT and archive import still call the first two until ticket 02. Core gained `SkipCount::tally` for merging skip reasons.
