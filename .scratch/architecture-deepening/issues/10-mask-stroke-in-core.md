# 10: Mask stroke in core

**What to build:** A brush, eraser or fuzzy fill stroke is a core module working on bitmaps and the image's annotations: painting and erasing along a segment, limited to the stroke's region, merging with same-label masks, skipping locked and hidden masks, removing masks erased to nothing. Finishing it returns the edit plus which annotations it removed, so the editor clears exactly those from the selection instead of guessing from the list length. The editor keeps only drawing the stroke's sprites on the canvas. This puts the logic beside the other image-analysis algorithms in core, as ADR 0002 intends.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] The stroke's mask logic compiles and is tested outside the browser
- [ ] Core tests cover: merge into the lowest-index same-label mask, locked and hidden masks untouched, erase to empty removes the mask, removals ordered so indices stay valid, the stop-at-edges limit
- [ ] The editor clears the selection from the stroke's reported removals
- [ ] Browser brush, eraser and fuzzy fill tests still pass
