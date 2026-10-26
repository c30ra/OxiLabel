# 09: One export footprint region, in core

**What to build:** The area a polyline or points annotation exports as (its export footprint) is computed in core, next to the label's line width and point radius. The exporters write it, and the editor's footprint ghost draws the same region, so what the annotator sees is exactly what is exported, pixel for pixel, including a one-point polyline (a dot) and pixel-centre snapping.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Exporters and the ghost both use the core footprint region; neither has its own footprint geometry
- [ ] A one-point polyline shows a dot in the ghost, as it does in the export
- [ ] Core tests fix the region for a polyline, a one-point polyline, round and square points, and an export-time width override
- [ ] Existing exporter tests pass unchanged; the browser footprint ghost test still passes
