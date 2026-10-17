# 12: YOLO OBB and YOLO segment exporters

**What to build:** Two more Exporters on the existing interface: YOLO oriented boxes and YOLO segmentation.

**Blocked by:** 06, 08, 09

**Status:** done

- [x] YOLO OBB export writes oriented boxes as normalized corner coordinates
- [x] YOLO segment export writes polygons, and converts brush masks to polygons
- ~~Mask PNG export writes per-image masks with label ids, a background value, and 255 for ignore regions~~ (dropped 2026-10-17)
- [x] Polylines and points are expanded (for YOLO segment) with per-label line widths and point radii set on each label, matching the existing `cvat_to_masks.py` behavior
- [x] Formats tests compare output against fixtures derived from the author's existing scripts

**Decisions (2026-10-17):** the mask PNG exporter is dropped (the custom pipeline keeps making its masks). Line widths and point radii live on each label (Labels tab), in original-image pixels, defaults 7 and 8 as in `cvat_to_masks.py`. YOLO segment expands polylines and points with them, like the masks did.

**Implementation note:** `rtag-formats/src/regions.rs` turns each annotation into the areas it covers: shapes with corners exactly; masks, polylines and points traced along their pixels (`Bitmap::outlines`, holes dropped, simplified with Douglas–Peucker for segment). Lines and points are drawn as OpenCV draws them in `cvat_to_masks.py`, at pixel centres. OBB boxes are the exact rectangle for (oriented) rectangles, the bounding square for circles, and the minimum-area rectangle of the traced pixels otherwise. `tests/yolo_segment_obb.rs` compares areas with numbers from running the script's `draw_shape` (`tests/fixtures/cvat_to_masks_reference.py`). OpenCV's thick lines vary by about a pixel in width with their angle, so a slanted line's area can differ by up to 15%. Labels gained `line_width` and `point_radius` (migration 0008, Labels tab). Mask PNG export was dropped.
