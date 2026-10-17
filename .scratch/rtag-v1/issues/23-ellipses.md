# 23: Ellipses replace circles

**What to build:** The circle shape becomes an ellipse: a centre, two radii and a rotation angle, so CVAT ellipses (ticket 16) import without loss. The Circle tool becomes the Ellipse tool (C, the tool pie's west slot): it keeps its two modes (drag from the centre, or across the bounding box), Shift makes a circle, Alt draws from the centre. A selected ellipse shows a handle for each radius and a rotation handle like oriented rectangles. Stored circles and labels allowing circles become ellipses.

**Blocked by:** 08

**Status:** ready-for-agent

- [ ] I can draw ellipses from the centre or across their box, and Shift draws a circle
- [ ] A selected ellipse can be resized along each radius and rotated by its handles
- [ ] Existing circle annotations and labels' allowed circle shape become ellipses
- [x] Exports (YOLO detect, segment, OBB) handle rotated ellipses

**Decisions (2026-10-17):** asked for while planning ticket 16 ("convert circle to ellipses in our program first"): ellipses rotate, with a handle; the tool is renamed Ellipse with Shift for a circle; stored circles migrate to ellipses with equal radii.

**Implementation note:** `rtag-core/src/ellipse.rs` holds the geometry (`Ellipse { centre, rx, ry, angle }`, the angle measured like `OrientedRect`'s); `Shape::Ellipse` replaces `Shape::Circle` and `ShapeType::Ellipse` replaces `Circle`. The tool draws the ellipse filling the box a drag spans, through the same `Rect::from_drag` as rectangles (Shift square, Alt from the centre), then shrinks it around its centre to fit the image, so a circle stays round at an edge. A selected ellipse has a centre handle (moves it), a handle at the end of each radius and a rotation handle above it (Shift snaps to 15°); a radius stops at the largest that fits. Migration `0011_ellipses.sql` rewrites stored circles as ellipses with equal radii and `"circle"` in labels' shape types as `"ellipse"` (checked on a copy of the dev database; old circle JSON is not read any more). YOLO segment exports the outline (36 corners), OBB the box along the ellipse's axes, detect its turned bounds.
