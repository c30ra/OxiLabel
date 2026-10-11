# 05: Rectangle annotation

**What to build:** In the viewer I can draw a rectangle Annotation with a chosen Label, select it, drag its handles, delete it, and undo/redo. Changes autosave per image as a batch, and a per-image version number rejects a save when the image changed meanwhile. Selection with the Select tool is the default.

**Blocked by:** 02, 04

**Status:** ready-for-agent

- [ ] Drawing a rectangle with two points creates an annotation stored in original image coordinates
- [ ] Click selects the topmost shape, handles resize it, Delete removes it, and Esc deselects
- [ ] Ctrl+Z and Ctrl+Y undo and redo within the image using a client-side command history
- [ ] Annotations autosave when I change or leave the image, with a visible saved, unsaved or failed indicator
- [x] A save based on a stale version is rejected with a clear message and nothing is overwritten
- [x] The image grid shows each image's annotation count (moved from ticket 03)
- [x] Deleting a label that is used by annotations tells me how many annotations would be removed and requires confirmation (moved from ticket 02)
- [x] Unit tests cover shape geometry and the command history, and API tests cover save and version conflicts

**Implementation note:** annotations live in an `annotations` table (shape as JSON in original-image coordinates, drawing order in `position`); images carry a `version`. `PUT /api/projects/{p}/images/{i}/annotations` with `{base_version, annotations}` replaces the whole set and bumps the version, or answers 409 if the version moved on (checked before validation, so a stale editor is told to reload first); labels, allowed shape types and image bounds are validated. `GET …/labels/{id}/usage` counts annotations; deleting a used label needs `?confirm=true`, bumps the affected images' versions and cascades. Core: `Rect::from_drag` (Shift square, Alt from centre), nearest-handle lookup, resize/move within the image, `topmost_at`, and `History` (add/remove/replace edits). UI: tool rail (Select V, Rectangle R), label picker in the top bar (label hotkeys switch it), drag or two-click drawing (the new shape is not selected afterwards, per review), click/drag/handles in Select, Delete/X removes, Esc cancels → deselects → back to Select, Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z). Autosave 1 s after the last change and on leaving the image; the top bar shows Saved / Unsaved changes / Saving… / Save failed (Retry) / Changed elsewhere (Reload); closing the tab with unsaved work asks first. The annotation canvas exposes `data-count`, `data-selected` and `data-revision` for browser tests.

