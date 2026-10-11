# 21: Edge snapping for freehand lines and the brush

**What to build:** Freehand drawing and the brush can follow edges in the image, like GIMP's intelligent scissors. While dragging a freehand polyline with edge snapping on, the line follows the strongest nearby edge (live-wire: a cheapest path over an edge-strength cost map between the last anchor and the pointer) instead of the exact pointer path. The brush can be limited to a region bounded by detected edges, so a stroke fills up to a border without spilling over, with an optional offset that grows or shrinks the region by a number of pixels. Both are options of their tools (tool settings bar and options pie), off by default.

**Blocked by:** 08, 09

**Status:** ready-for-agent

- [ ] Freehand polylines have an "Snap to edges" option: vertices follow the strongest edge near the pointer, still evenly spaced, and Shift-free drawing is unchanged when the option is off
- [ ] The brush has a "Stop at edges" option: paint stays inside the similarity region grown from the stroke start (the same region growing and threshold as the fuzzy fill, ticket 22), with an optional offset in pixels (positive grows the region, negative shrinks it)
- [ ] Analysis runs in the browser on lossless tiles of the viewed zoom level (ADR 0002); a zoom change mid-stroke applies from then on, and 6 MP+ images stay responsive
- [ ] Snapping strength or search radius is adjustable, and a stroke can be drawn without snapping by holding a modifier
- [ ] The tools analyse the original image; a per-tool switch to analyse the active enhancement filter's view is added with ticket 13
- [ ] Edge detection and the live-wire path search are unit-tested in the core crate on synthetic images, and a browser test covers drawing along a synthetic edge

**Notes:** requested 2026-10-11. Decisions (grilling 2026-10-11, ADR 0002): computation in the browser with algorithms in `rtag-core`; the viewed level, not always full resolution; lossless PNG analysis tiles generated on first use per image; the original image by default with a per-tool switch for the filtered view; the edge-limited brush is bounded by a similarity region (shared with ticket 22), while live-wire uses an edge map. Build on the original image after ticket 09.
