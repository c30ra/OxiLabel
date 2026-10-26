# 11: Editor session: the Select tool's gestures, cancelled by list changes

**What to build:** A plain-Rust editor session owns an image's annotations, undo history, selection and the gesture in progress, and is driven by input events (press, move, release, key action, undo, redo, merging a pre-annotation run) carrying modifiers and the view scale. This ticket moves the Select tool into it: picking, box selection, moving the selection, resizing rectangles, dragging, inserting and removing vertices, turning oriented rectangles and ellipses, and dragging ellipse radii. Which handles a shape has (none for a fixed-size shape) comes from one list used for both hit-testing and drawing. Undo, redo or a merged pre-annotation run during a gesture that holds an annotation index cancels it, so it can never change the wrong annotation. The annotation layer becomes the adapter: browser events in, signals and canvas drawing out.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Every Select-tool gesture runs through the session; the annotation layer holds no Select gesture rules
- [ ] Hit-testing and drawing handles use the same handle list
- [ ] Ctrl+Z during a vertex drag, and a run merged during a move, leave the annotations unchanged and the gesture cancelled
- [ ] Rust tests drive the session for each Select gesture, including the undo-during-drag and merge-during-drag cases
- [ ] Browser Select, vertex and box selection tests still pass
