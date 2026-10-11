# 22: Fuzzy fill (magic wand) for masks

**What to build:** A fuzzy fill for the brush, like GIMP's fuzzy select ("magic wand"): clicking a pixel fills the mask with the region around it whose colour or brightness is within a threshold of the clicked pixel, so a defect or area of similar intensity is annotated in one click. The threshold is adjustable (tool settings bar and options pie, and by dragging while clicking, as in GIMP), and the fill can be contiguous (only connected pixels, the default) or global (every similar pixel in the image or the visible area). The result is added to the current mask, or removed from it in eraser mode.

**Blocked by:** 09

**Status:** needs-triage

- [ ] A click with the fuzzy fill adds the connected region of similar pixels to the mask with the current label
- [ ] The threshold can be set numerically and by dragging after the click, with a live preview of the region before it is committed
- [ ] Contiguous and global modes; in eraser mode the region is removed from the mask
- [ ] Similarity is measured on the original image by default, with an option to use the active enhancement filter's view (ticket 13) for faint structures
- [ ] Optional smoothing of the result (fill small holes, ignore regions smaller than N pixels) and a maximum region size to stop runaway fills
- [ ] Each fill is one undo step, works on 6 MP+ images without noticeable lag, and the region growing is unit-tested in the core crate on synthetic images

**Notes:** requested 2026-10-11. Related to ticket 21 (edge-limited brush): both need fast pixel access to the original image in the browser or on the server; triage should decide where that computation lives for both.
