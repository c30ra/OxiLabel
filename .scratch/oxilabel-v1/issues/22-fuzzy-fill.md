# 22: Fuzzy fill (magic wand) for masks

**What to build:** A fuzzy fill for the brush, like GIMP's fuzzy select ("magic wand"): clicking a pixel fills the mask with the region around it whose colour or brightness is within a threshold of the clicked pixel, so a defect or area of similar intensity is annotated in one click. The threshold is adjustable (tool settings bar and options pie, and by dragging while clicking, as in GIMP), and the fill can be contiguous (only connected pixels, the default) or global (every similar pixel in the image or the visible area). The result is added to the current mask, or removed from it in eraser mode.

**Blocked by:** 09

**Status:** done

- [x] A click with the fuzzy fill adds the connected region of similar pixels to the mask with the current label
- [x] The threshold can be set numerically and by dragging after the click, with a live preview of the region before it is committed
- [x] Contiguous and global modes; in eraser mode the region is removed from the mask
- [x] Similarity is measured on the original image; a switch to use the active enhancement filter's view is added with ticket 13
- [x] Analysis runs in the browser on lossless tiles of the viewed zoom level (ADR 0002); zooming in gives a finer region
- [x] Optional smoothing of the result (fill small holes, ignore regions smaller than N pixels) and a maximum region size to stop runaway fills
- [x] Each fill is one undo step, works on 6 MP+ images without noticeable lag, and the region growing is unit-tested in the core crate on synthetic images

**Notes:** requested 2026-10-11. Decisions (grilling 2026-10-11, ADR 0002): computation in the browser with region growing in `oxilabel-core`, at the viewed level, on lossless PNG analysis tiles generated on first use per image; the original image by default with a per-tool switch for the filtered view. The same region growing bounds the edge-limited brush of ticket 21.

**Implementation note (2026-10-12):** decisions asked of the user: the fill is a brush mode ("Paint | Fuzzy fill" in the settings bar; the options pie's north item; U switches to it, B back to painting; the eraser toggle and Ctrl invert apply to it too), and it grows tile by tile, fetching only the neighbouring analysis tile a region reaches, until the region stops. Server: `/api/images/{id}/analysis/{level}/{x}/{y}` serves the image pyramid as lossless PNG, generated on first use (`tiles.rs`, `Kind::Analysis`). Core: `oxilabel_core::fill::grow` (breadth-first over the loaded tiles of one level; similarity is the largest channel difference from the clicked pixel; names the unloaded tiles it reached; `within` makes it global over an area; stops at `max_pixels`), plus `Bitmap::cleaned` (drops patches and fills holes smaller than N), `scaled_up` and `subtract`. UI (`analysis.rs`, `annotate.rs`): a click grows on the level being viewed, fetches missing tiles and grows again as each arrives, previewing the region; dragging right or down raises the threshold (0.5 per screen pixel), and `[`/`]` or the options pie change it by 5; release commits as one undo step once growing is complete, merging into touching masks of the label like a brush stroke (eraser mode cuts every mask). Implementer's choices: global mode covers the visible area; a fill that reaches the maximum size (default 25% of the image) is not added, with a notice, because it would be a cut-off part of a larger region; clean-up never removes the whole fill. Measured in headless Zen on a 6240 × 4160 image: a threshold drag over a 400k-pixel (capped) region takes about 18 ms per pointer move; a fill at 151% that loads level-0 tiles as it grows finishes in about 1 s plus the autosave delay.
