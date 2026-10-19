# 04: Tile-pyramid viewer

**What to build:** Opening an Image from the grid shows it in a canvas workspace with scroll-wheel zoom and Space-drag pan. The server cuts the image into a multi-level tile pyramid lazily on first view and caches it on disk, and the viewer loads finer tiles as I zoom in.

**Blocked by:** 03

**Status:** done

- [x] A 6 MP image opens quickly and stays responsive while panning and zooming
- [x] Tiles are generated on first request and served from cache afterwards
- [x] Zooming to 100% and beyond shows full-resolution detail
- [x] A coordinate transform between screen and original image coordinates exists in the core crate and is unit-tested
- [x] API tests cover tile generation, caching and out-of-range requests

**Implementation note:** `oxilabel_core::pyramid` (512 px tiles, level 0 = full resolution, each level halves until one tile fits) and `oxilabel_core::view` (screen = image × scale + offset; fit, zoom around a point, pan) are shared by server and UI. `GET /api/images/{id}/tiles/{level}/{x}/{y}` builds the whole pyramid on the first request for an image (decode once, 2×2-average each level, JPEG q90 encoded on all cores, a per-hash lock so concurrent requests generate once) under `OXILABEL_CACHE_DIR/tiles/{hash}/original/`, then serves files; enhancement filters (ticket 13) get sibling folders. Measured on the author's data (12 cores): first view 0.26 s at 14 MP, about 0.35 s at 26 MP, 0.43 s at 33 MP; cached tiles about 1 ms. The workspace (`/projects/:id/images/:image`) draws coarse tiles first and finer ones as they arrive; wheel zooms around the cursor, Space-drag or middle-drag pans, Home fits, + and − zoom, the zoom button toggles 100% / fit, and pixels stay crisp past 100%. Clicking a grid tile opens it (unless something is selected). UI files are now served with `Cache-Control: no-cache` so a rebuild is never masked by a stale cached build.

