# 02: CVAT import, project archive import and label edits write through the Annotation store

**What to build:** Every remaining writer of annotations goes through the Annotation store from 01: CVAT import (append, or replace an image's annotations), project archive import (a new image's annotations), and label changes that touch stored annotations (conforming attribute values, deleting a label). The rules the store enforces then hold for every path: a project archive with two copies of one tag on an image is refused with a clear reason instead of stored, and narrowing a label's allowed shape types is refused while annotations of the removed shape types exist, instead of leaving images that fail their next save. The annotated status still changes only on an editor save.

**Blocked by:** 01

**Status:** ready-for-agent

- [x] No module except the store inserts, deletes or re-positions annotation rows, or bumps an image's version for an annotation change
- [x] Importing a project archive whose image has a duplicate tag reports the problem and stores no duplicate
- [x] Removing a shape type from a label that still has annotations of that type is refused with a message naming how many annotations use it
- [x] Store tests cover the append path (positions after existing annotations, tags already on the image skipped)
- [x] Existing CVAT import, archive and label HTTP tests still pass

## Comments

**Implementation (ad/02):** all in `crates/oxilabel-server/src/annotation_store.rs`.

- `Replacing::Nothing` is the append variant (CVAT append); CVAT replace and archive import write with `Replacing::All`. A write that removes and stores nothing no longer bumps the version.
- `Written.left_out` gives the positions of the annotations the store left out, so CVAT counts dropped attribute values only for shapes it stored (as before).
- `preview(conn, image, size, labels, replacing, annotations)` sorts out like `write` but writes nothing, against labels the caller passes: CVAT previews use it, since previewed labels and images do not exist yet.
- `fit_to_label` (was `labels::conform_attribute_values`) runs after a label edit, in its transaction: it refuses (409) while annotations have a shape type the label no longer allows ("label \"x\" still has 2 annotations with rectangle shapes; delete them before removing that shape type"), then conforms attribute values and bumps the changed images' versions.
- `remove_label_annotations` deletes a label's annotations and bumps their images' versions before `labels.rs` deletes the label row.
- `check_annotation` and `insert_annotations` are private now; `cvat.rs`'s `Skipped` is gone (`SkipCount::tally`).
