# 04: Pure CVAT import planner

**What to build:** A CVAT import is planned before anything is written. A pure planner takes the parsed CVAT export, the project's labels and images, the folder search results and the user's choices, and returns the import report plus the list of labels, images and annotations to create. The preview shows that plan; committing applies it in one transaction (new labels and images first, then annotations through the Annotation store). The preview no longer stands in placeholder ids for labels and images it would create, and preview and commit cannot disagree because they share the plan.

**Blocked by:** 02

**Status:** ready-for-agent

- [x] Preview and commit both come from the same plan; no placeholder ids remain
- [x] Planner tests (no database, no multipart) cover: label matched by name ignoring case, created, or skipped; image matched by name and size, ambiguous, size mismatch, found in the chosen folder; attributes and tags filtered; skipped reasons counted
- [x] Most CVAT HTTP tests become planner tests; one or two HTTP tests keep covering preview and commit end to end
- [x] Browser CVAT import flow still works

## Comments

`crates/oxilabel-server/src/cvat_plan.rs` holds the pure planner (`plan`) and its unit tests. It
returns a `CvatPlan` with the report, new `LabelDefinition`s, and per-image planned annotations
whose labels are `LabelRef::{Existing(id), New(index)}` (the image side is
`ImageTarget::{Existing, New}`): no negative or placeholder ids anywhere. The handler loads labels,
images (with their tag labels) and found files, plans once, and either returns the plan's report or
applies it: insert all new labels, then all new images, then write every image's planned
annotations through the annotation store (`Replacing::{All, Nothing}`), checking the store stored
exactly what the plan said. `annotation_store::preview` and `Written.left_out` were removed as
obsolete.

Annotation validation now lives in core (`Label::check_annotation` and
`LabelDefinition::check_annotation`, sharing one implementation); the store and the planner both
use it, so preview and commit cannot disagree. HTTP tests were reduced to
`preview_reports_the_shared_plan_without_writing` and
`commit_applies_the_plan_and_the_annotation_store_rules`; their label/image/attribute cases moved
to planner tests. `scripts/ui-test.sh CVAT` passes.
