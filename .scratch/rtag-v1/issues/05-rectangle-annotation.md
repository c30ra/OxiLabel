# 05: Rectangle annotation

**What to build:** In the viewer I can draw a rectangle Annotation with a chosen Label, select it, drag its handles, delete it, and undo/redo. Changes autosave per image as a batch, and a per-image version number rejects a save when the image changed meanwhile. Selection with the Select tool is the default.

**Blocked by:** 02, 04

**Status:** ready-for-agent

- [ ] Drawing a rectangle with two points creates an annotation stored in original image coordinates
- [ ] Click selects the topmost shape, handles resize it, Delete removes it, and Esc deselects
- [ ] Ctrl+Z and Ctrl+Y undo and redo within the image using a client-side command history
- [ ] Annotations autosave when I change or leave the image, with a visible saved, unsaved or failed indicator
- [ ] A save based on a stale version is rejected with a clear message and nothing is overwritten
- [ ] Unit tests cover shape geometry and the command history, and API tests cover save and version conflicts
