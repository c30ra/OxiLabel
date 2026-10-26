# 09: One export footprint region, in core

**What to build:** The area a polyline or points annotation exports as (its export footprint) is computed in core, next to the label's line width and point radius. The exporters write it, and the editor's footprint ghost draws the same region, so what the annotator sees is exactly what is exported, pixel for pixel, including a one-point polyline (a dot) and pixel-centre snapping.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [x] Exporters and the ghost both use the core footprint region; neither has its own footprint geometry
- [x] A one-point polyline shows a dot in the ghost, as it does in the export
- [x] Core tests fix the region for a polyline, a one-point polyline, round and square points, and an export-time width override
- [x] Existing exporter tests pass unchanged; the browser footprint ghost test still passes

## Comments

`Footprint::region(&self, &Shape, image) -> Option<Bitmap>` in `crates/oxilabel-core/src/lib.rs` (next to the line width and point radius) gives the pixels a polyline or points shape exports as: pixel-centre snapping, a one-point polyline as a dot, OpenCV-like line reach, disc or square points, clipped to the image; `None` for other shapes. `line_reach`/`point_reach` are now private to it, and the OpenCV notes moved from `oxilabel-formats/src/regions.rs` to the `Footprint` docs. `ExportRequest::footprint_of(&Label)` (core `export.rs`) picks an export's validated override or the label's own; the server's export handler calls it instead of its own loop. `regions.rs` traces `footprint.region(..)`; the editor ghost (`draw::footprint`) now draws the region's `Bitmap::outlines()` (fill plus a 1.5 px outline outside, overlaps sharing one outline) instead of stroking the line/discs itself; `annotate.rs` caches the outlines per annotation index and remakes them only when the shown shape or footprint changes. Core tests: `crates/oxilabel-core/tests/footprint.rs`. No browser test for the one-point dot: annotations need two different points to be saved through the API, so the dot is covered by the core test of the region the ghost draws.
