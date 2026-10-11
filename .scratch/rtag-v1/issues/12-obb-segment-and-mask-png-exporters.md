# 12: YOLO OBB, YOLO segment and mask PNG exporters

**What to build:** Three more Exporters on the existing interface: YOLO oriented boxes, YOLO segmentation, and label-driven mask PNGs. The mask exporter writes each label's permanent id and the ignore value for regions marked with an ignore label.

**Blocked by:** 06, 08, 09

**Status:** ready-for-agent

- [ ] YOLO OBB export writes oriented boxes as normalized corner coordinates
- [ ] YOLO segment export writes polygons, and converts brush masks to polygons
- [ ] Mask PNG export writes per-image masks with label ids, a background value, and 255 for ignore regions
- [ ] Polylines and points are expanded to masks with per-label widths and radii configurable in the export settings, matching the existing `cvat_to_masks.py` behavior
- [ ] Formats tests compare output against fixtures derived from the author's existing scripts
