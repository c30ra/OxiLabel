# 01: Annotation store, with editor saves and pre-annotation runs as its first callers

**What to build:** One server module owns every change to an image's set of annotations, so the rules for that change live in one place. Its first two callers are the editor's save (replace the whole set, only if the version still matches) and a pre-annotation run (replace one model's pending pre-annotations on an image). The store checks each annotation against its label and the image size, allows one copy of each tag per image, assigns positions, and bumps the image version so a stale editor reloads. Only the editor save sets the image's annotated status from `counts_as_annotated`; ticket 26 (oxilabel-v1) decided that imports, pre-annotation runs and label deletes leave the status as it is, and the store keeps that rule. The store runs inside the caller's transaction.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Editor saves and pre-annotation runs write annotations only through the store; their own INSERT, position and version SQL is gone
- [ ] The shallow insert helper the writers called is no longer used by them
- [ ] A pre-annotation run can no longer add a second copy of a tag the image already has (it is reported as skipped)
- [ ] Store tests on in-memory SQLite cover each operation against each rule: per-annotation check, one tag per label, positions, version bump, annotated status only on save, stale version refused
- [ ] Existing annotation and pre-annotation HTTP tests still pass
