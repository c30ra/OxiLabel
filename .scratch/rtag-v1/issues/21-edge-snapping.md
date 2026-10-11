# 21: Edge snapping for freehand lines and the brush

**What to build:** Freehand drawing and the brush can follow edges in the image, like GIMP's intelligent scissors. While dragging a freehand polyline with edge snapping on, the line follows the strongest nearby edge (live-wire: a cheapest path over an edge-strength cost map between the last anchor and the pointer) instead of the exact pointer path. The brush can be limited to a region bounded by detected edges, so a stroke fills up to a border without spilling over, with an optional offset that grows or shrinks the region by a number of pixels. Both are options of their tools (tool settings bar and options pie), off by default.

**Blocked by:** 08, 09

**Status:** needs-triage

- [ ] Freehand polylines have an "Snap to edges" option: vertices follow the strongest edge near the pointer, still evenly spaced, and Shift-free drawing is unchanged when the option is off
- [ ] The brush has a "Stop at edges" option that keeps paint inside the edge-bounded region under the stroke start, with an optional offset in pixels (positive grows past the edge, negative stays inside)
- [ ] Edges are detected on the original image (not the view filter) at the resolution being viewed, computed lazily per visible area so 6 MP+ images stay responsive
- [ ] Snapping strength or search radius is adjustable, and a stroke can be drawn without snapping by holding a modifier
- [ ] Edge detection and the live-wire path search are unit-tested in the core crate on synthetic images, and a browser test covers drawing along a synthetic edge

**Notes:** requested 2026-10-11 while reviewing freehand polylines. Open questions for triage: whether edges should optionally come from the active enhancement filter (ticket 13), and whether the computation runs in the browser (WASM, on the loaded tiles) or on the server.
