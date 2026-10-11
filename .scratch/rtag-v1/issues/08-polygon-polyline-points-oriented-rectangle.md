# 08: Polygon, polyline, points, oriented rectangle

**What to build:** The remaining vector shapes become available through the pie menu: polygon, polyline, points and oriented rectangle (3-point). Each supports vertex editing and the image-editor constraints, with per-tool options in the options ring.

**Blocked by:** 07

**Status:** ready-for-agent

- [ ] Polygons and polylines are drawn by clicking vertices and finished with Enter or double-click
- [ ] Oriented rectangles are drawn with three points, and rectangles can also be drawn from the center with Alt
- [ ] Shift constrains lines to fixed angles (15 degrees) and rectangles to squares
- [ ] Vertices can be added, removed and dragged on existing shapes
- [x] Labels restricted to certain shape types cannot be used with other shapes
- [x] Geometry (including oriented rectangle math) is unit-tested, and annotations persist and reload correctly

**Implementation note:** core `Shape` gains `OrientedRectangle(OrientedRect)` (centre, width, height, angle in radians; built from three points), `Polygon`, `Polyline` and `Points` (`{points}`), with bounds, hit-testing (polygons inside or near an edge, polylines near a segment, points near a point), translation, validation (inside the image, minimum vertices, non-degenerate) and vertex operations (`with_vertex`, `with_inserted_vertex`, `without_vertex` down to 3/2/1). `geometry::snap_angle` snaps to 15° steps. UI: tools Polygon (P), Polyline (L), Points (.) in the rail and the tool pie; clicks place vertices, Enter or a double-click finishes (a polygon also by clicking its first vertex), Backspace drops the last vertex, Shift (or the "Snap 15°" option) snaps lines. The rectangle options pie switches between 2-point and 3-point oriented (Shift snaps the first side). With one shape selected: drag a vertex, drag an edge's round "+" to insert one, Ctrl+click a vertex to remove it; oriented rectangles have corner handles (resize, keeping the angle) and a rotation handle (Shift snaps 15°). Switching tool picks a label that allows its shape; hotkeys and the label picker refuse labels that don't. YOLO detect exports every shape as its bounding box. Browser tests in `scripts/ui-tests` cover each tool and edit.

