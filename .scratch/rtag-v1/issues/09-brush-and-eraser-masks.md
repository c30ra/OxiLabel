# 09: Brush and eraser masks

**What to build:** A brush and eraser tool paint mask Annotations on an image. Brush size changes with `[` and `]`, masks are stored as RLE in original image coordinates, and they appear in the objects and in saved state like any other shape.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] I can paint and erase a mask with a chosen label, with resizable brush size shown as a cursor outline
- [x] Masks are stored as RLE in original image coordinates and reload identically
- [ ] Painting works while zoomed in and at multi-megapixel image sizes without noticeable lag
- [x] Undo and redo work for brush strokes
- [x] RLE encode and decode and mask editing operations are unit-tested in the core crate

**Note (2026-10-11):** the tool pie's west slot now holds Circle (ticket 08 follow-up), so the eraser is a Brush option, as in Blender: an "Eraser" toggle in the brush's tool settings bar and options pie, with E switching straight to the brush in eraser mode. Edge-limited painting is ticket 21.

**Implementation note (2026-10-12):** decisions asked of the user: a stroke extends every mask of the current label it touches (merging the ones it joins, kept at the lowest one's place) or else becomes a new mask; the eraser removes pixels from every mask under it, whatever its label (emptied masks are removed); masks of different labels may overlap. Core: `rtag_core::mask` has `Mask` (bounding box `left`/`top`/`width`/`height` in image pixels and `rle`: row-major runs alternating unset and set, starting with unset), `Bitmap` (decoded; `paint`/`erase` a round brush along a segment, pixel centres within the radius; `merge`, `overlaps`, `to_mask` crops) and `Shape::Mask` (no vertices; bounds from the box; hit-testing by pixel; moves by whole pixels). UI: Brush (B, tool pie south-west) with Brush/Eraser and Size (diameter in image pixels, default 20, `[`/`]` by 25%) in the tool settings bar and options pie; E switches to the brush in eraser mode. The pointer shows the brush outline. Masks are drawn from offscreen canvases cached by their pixels and colour, and a stroke updates only the pixels it touched (`crates/rtag-ui/src/masks.rs`); each stroke is one undo step, and Ctrl+Z/Y are ignored mid-stroke. Measured in headless Zen on a 6240 × 4160 image: about 3 ms per pointer move (5 ms at worst), fitted and at 112%. Browser tests cover painting, extending, joining, overlapping labels, erasing, undo/redo, selecting and moving masks.

**Follow-up (2026-10-12):** paint-program modifiers, as chosen by the user: Ctrl (or Cmd) held when a stroke starts inverts it (the brush erases, the eraser paints), and F starts Blender's radial size control (the outline keeps its size with the pointer on its edge, then follows the pointer; a click, F or Enter confirms, Esc or right-click cancels). E stays a plain switch to eraser mode. Covered by a browser test.
