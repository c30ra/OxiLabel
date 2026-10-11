# Image-analysis aids run in the browser at the viewed resolution, on lossless tiles

Edge snapping (live-wire), fuzzy fill and the edge-limited brush analyse pixels in the browser (WASM, algorithms in `rtag-core`) on the tiles of the zoom level being viewed, so they can react at pointer speed; a server round trip per pointer move or threshold change would be too slow. The analysed pixels come from a second, lossless (PNG) tile pyramid, generated the first time a tool needs it for an image, because the viewing tiles are JPEG and their block edges would mislead edge detection and low thresholds.

## Considered Options

- Server-side analysis on full-resolution pixels: exact, but too slow for live-wire and live threshold dragging.
- Always full resolution in the browser (fetching level-0 tiles around the work area): zoom-independent results, rejected in favour of the simpler, faster view-level analysis.
- Browser preview plus a server recompute on release: twice the work, and the result could differ from the previewed one.

## Consequences

- Results depend on zoom: the same click gives a coarser region when zoomed out; zoom in for precision. A zoom change in the middle of a stroke or fill applies from that moment on; what is already drawn stays as computed.
- Each image can have a lossless analysis pyramid beside its viewing pyramid (and, once enhancement filters exist, one per filtered view that is analysed).
- The tools analyse the original image by default; each tool has its own switch to analyse the active enhancement filter's view instead.
