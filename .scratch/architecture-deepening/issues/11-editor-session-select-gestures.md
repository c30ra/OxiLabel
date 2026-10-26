# 11: Editor session: the Select tool's gestures, cancelled by list changes

**What to build:** A plain-Rust editor session owns an image's annotations, undo history, selection and the gesture in progress, and is driven by input events (press, move, release, key action, undo, redo, merging a pre-annotation run) carrying modifiers and the view scale. This ticket moves the Select tool into it: picking, box selection, moving the selection, resizing rectangles, dragging, inserting and removing vertices, turning oriented rectangles and ellipses, and dragging ellipse radii. Which handles a shape has (none for a fixed-size shape) comes from one list used for both hit-testing and drawing. Undo, redo or a merged pre-annotation run during a gesture that holds an annotation index cancels it, so it can never change the wrong annotation. The annotation layer becomes the adapter: browser events in, signals and canvas drawing out.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] Every Select-tool gesture runs through the session; the annotation layer holds no Select gesture rules
- [x] Hit-testing and drawing handles use the same handle list
- [x] Ctrl+Z during a vertex drag, and a run merged during a move, leave the annotations unchanged and the gesture cancelled
- [x] Rust tests drive the session for each Select gesture, including the undo-during-drag and merge-during-drag cases
- [x] Browser Select, vertex and box selection tests still pass

## Comments

Implemented in `crates/oxilabel-core/src/editor/` (plain Rust, tested by `cargo test -p oxilabel-core editor::`):

- `mod.rs`: `Session` owns the annotations, the undo history, the selection, the gesture, the labels (for fixed sizes), the image size, the pre-annotation threshold and the snap option. It takes `Pointer { at, scale, modifiers }` events and `key(Action)`; the list operations the UI `Editor` had (`apply`, `undo`, `redo`, `reset`, `remove`, `delete_selected`, `set_flag`, `change_selected`, `merge_pre_annotations`, `accept`, `pending`, `remove_all`) moved here. Every list change goes through one place that cancels a gesture holding annotation indices, so undo, redo, a merged run, a reload or a Delete during a drag cancel it.
- `select.rs`: the Select gestures (picking, Shift/Ctrl clicks, box selection, moving, resizing, vertex drag/insert/remove, turning oriented rectangles and ellipses, ellipse radii), and what the layer draws (`shown_shape`, `selection_box`, `grips`).
- `grip.rs`: the one handle list per shape (none that resize for a fixed-size shape), used both for hit-testing and by the new `draw::grip`, which replaces the per-shape handle drawing. `Rect::handle_at` went (hit-testing is now by distance to the listed handles). Locked shapes show no handles, as they had none to grab.
- UI: `Editor` keeps the session in a `StoredValue` with triggers; `editor.update(|s| ..)` changes it and notifies, tracked getters (`with_annotations`, `selection`, `revision`, `threshold`, `pending`) replace the `annotations`, `selection`, `revision` and `pre_annotation_threshold` signals. The annotation layer only forwards Select events and draws; the drawing tools' gestures stay in the layer until ticket 12.
- Tests: `crates/oxilabel-core/src/editor/tests/` (helpers in `mod.rs`, Select tests in `select.rs`).
