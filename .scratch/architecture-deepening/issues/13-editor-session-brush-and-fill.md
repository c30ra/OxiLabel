# 13: Editor session: brush and fuzzy fill gestures

**What to build:** Painting, erasing, resizing the brush with F, and fuzzy fill (seed, threshold drag, commit when growing completes) run through the editor session, using the core mask stroke from 10. Undo and redo during a stroke follow the session's one rule for gestures holding annotation indices, replacing the special case that only guards painting. The editor keeps the canvas sprites and the tile reading for analysis.

**Blocked by:** 10, 12

**Status:** done

- [x] Brush, eraser, F resizing and fuzzy fill gestures run through the session
- [x] The painting-only undo guard is gone; the session's rule covers it
- [x] Rust tests cover a stroke, Ctrl inverting it, F resize and Esc restoring the size, a fill's threshold drag and a fill committed after release
- [x] Browser brush and fuzzy fill tests still pass

## Comments

Branch `ad/13-editor-session-brush-and-fill`, commits `3d054a7` (the gestures and tests) and `bd38ca8` (merge of the integration tip, which had ticket 14). The merge picked up ticket 14's `BrushMode` and tool-switching rule; the session's `BrushMode` is the only source of the eraser/fill choice, and `Options` kept only the numeric brush options.

**Core (`crates/oxilabel-core/src/editor/brush.rs`)**
- `Gesture::Painting { stroke: MaskStroke, seed, limited }` starts from a press (Ctrl inverts the mode's eraser, a label must allow masks unless erasing), extends on every move through `MaskStroke::segment`, and commits one undo step on release (`StrokeOutcome::selection_after`, removals, label default attributes). `holds_indices` now lists `Painting`, so the existing list-change rule cancels a stroke on undo, redo, a merged run or a reload — the layer's painting-only guard is gone.
- `Gesture::BrushSizing { centre, before }` is the F gesture: `begin_brush_sizing` where the pointer is, the size follows the horizontal move (`[`/`]` and the options pie still step it), Esc restores `before`, F or Enter keep it.
- `Gesture::Filling { seed, anchor, base, erase, released, region }` seeds on press, changes the threshold from `base` by the drag (per screen pixel, using the view scale), and commits `MaskStroke::from_region` once released and `region.incomplete` is false. A capped region never commits; the notice says why.
- `Layer` carries what only the layer can supply: `edges` (freehand path), `fill` (grow the region at a threshold) and `limit` ("Stop at edges" pixels). `pixels_ready` grows the fill or limit again when tiles arrive or the view changes; `take_stroke_change` hands the layer the pixels each segment changed, for its sprites.
- Constants for the brush and fill live here (`DEFAULT_BRUSH_SIZE`, `MIN_BRUSH_SIZE`, `MAX_BRUSH_SIZE`, `BRUSH_STEP`, `DEFAULT_THRESHOLD`, `MAX_THRESHOLD`, `THRESHOLD_PER_PIXEL`, `THRESHOLD_STEP`); the UI re-exports them.

**UI (`crates/oxilabel-ui/src/annotate.rs`, `masks.rs`)**
- The layer's `Gesture` enum, its `MaskStroke`, `regrow`/`commit_fill` and `update_limit` are gone. Presses, moves and releases pass a `Layer` built from the analysis tiles; the sprites stay: `masks::Stroke` refreshes from the session's bitmaps per `SegmentChange`, and `FillPreview` is rebuilt from `Session::fill_preview()` only when the region changed.
- `Editor::update` mirrors a brush size or fill threshold the session changed (F, `[`/`]`, a threshold drag) back into the persisted tool signals, so the options bar and session storage stay right without the UI owning the gesture.
- The key handler has no painting guard: Ctrl+Z/Ctrl+Y go to the session, whose list rule cancels a stroke; Esc and the pointer handlers are adapters over the session's gestures.

**Tests**
- `cargo test --workspace --exclude oxilabel-ui` (from the worktree, after the merge): all suites pass; `crates/oxilabel-core/src/editor/tests/brush.rs` has 9 tests (one stroke = one undo step; Ctrl erases with the brush and paints with the eraser; F resize + Esc restore; threshold drag; commit on release; waiting for tiles; undo during a stroke drops it; a brush without a label paints nothing; Shift inverts stop-at-edges).
- `scripts/ui-test.sh` filters, after the merge: `brush` 4, `Ctrl inverts` 1, `stroke joining` 1, `fuzzy fill` 2, `filtered view` 1, `freehand` 2, `rectangle` 7, `tool settings bar` 1, `tool pie opens` 1, `drawing tool opens its options pie` 1, `tool rail switches` 1 — all passed, 0 failed. Before the merge the same brush, Ctrl/F, fuzzy fill, stop-at-edges and filtered-fill tests also passed.

**Limits:** growing the fill region, cleaning it up and growing the "Stop at edges" limit stay in the adapter, because they read the analysis tiles; the brush size and fill threshold remain persisted tool options, mirrored from the session. Fill gestures do not hold annotation indices (the region is committed against the list as it is then), so undo during a fill is allowed and the released fill commits afterwards.