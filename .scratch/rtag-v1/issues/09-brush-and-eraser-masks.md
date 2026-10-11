# 09: Brush and eraser masks

**What to build:** A brush and eraser tool paint mask Annotations on an image. Brush size changes with `[` and `]`, masks are stored as RLE in original image coordinates, and they appear in the objects and in saved state like any other shape.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] I can paint and erase a mask with a chosen label, with resizable brush size shown as a cursor outline
- [ ] Masks are stored as RLE in original image coordinates and reload identically
- [ ] Painting works while zoomed in and at multi-megapixel image sizes without noticeable lag
- [ ] Undo and redo work for brush strokes
- [ ] RLE encode and decode and mask editing operations are unit-tested in the core crate

**Note (2026-10-11):** the tool pie's west slot now holds Circle (ticket 08 follow-up), so the eraser is a Brush option, as in Blender: an "Eraser" toggle in the brush's tool settings bar and options pie, with E switching straight to the brush in eraser mode. Edge-limited painting is ticket 21.
