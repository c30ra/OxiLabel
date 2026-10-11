# 04: Tile-pyramid viewer

**What to build:** Opening an Image from the grid shows it in a canvas workspace with scroll-wheel zoom and Space-drag pan. The server cuts the image into a multi-level tile pyramid lazily on first view and caches it on disk, and the viewer loads finer tiles as I zoom in.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] A 6 MP image opens quickly and stays responsive while panning and zooming
- [ ] Tiles are generated on first request and served from cache afterwards
- [ ] Zooming to 100% and beyond shows full-resolution detail
- [ ] A coordinate transform between screen and original image coordinates exists in the core crate and is unit-tested
- [ ] API tests cover tile generation, caching and out-of-range requests
