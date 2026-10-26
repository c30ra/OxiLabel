# 10: Mask stroke in core

**What to build:** A brush, eraser or fuzzy fill stroke is a core module working on bitmaps and the image's annotations: painting and erasing along a segment, limited to the stroke's region, merging with same-label masks, skipping locked and hidden masks, removing masks erased to nothing. Finishing it returns the edit plus which annotations it removed, so the editor clears exactly those from the selection instead of guessing from the list length. The editor keeps only drawing the stroke's sprites on the canvas. This puts the logic beside the other image-analysis algorithms in core, as ADR 0002 intends.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] The stroke's mask logic compiles and is tested outside the browser
- [x] Core tests cover: merge into the lowest-index same-label mask, locked and hidden masks untouched, erase to empty removes the mask, removals ordered so indices stay valid, the stop-at-edges limit
- [x] The editor clears the selection from the stroke's reported removals
- [x] Browser brush, eraser and fuzzy fill tests still pass

## Comments

- New module `oxilabel_core::stroke`: `MaskStroke::begin(erase, label, size, image, at)`, `segment(to, &annotations) -> SegmentChange` (painted region / erased indices+regions for redrawing), `set_limit`/`limit(on)`, `from_region` (fuzzy fill), `finish(self, &annotations) -> Option<StrokeOutcome { edit, removed }>` (`removed` highest first) and `StrokeOutcome::selection_after(&selection)`. Accessors `painted()`, `erased(index)`, `is_merged(index)`.
- `oxilabel-ui/src/masks.rs` `Stroke` is now only the core stroke plus its sprites; the editor's `commit_stroke` maps the selection through `selection_after` instead of clearing it when the list shrank.
