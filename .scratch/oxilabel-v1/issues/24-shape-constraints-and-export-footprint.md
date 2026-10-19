# 24: Shape constraints and export footprint

**What to build:** A label can give each of its sizable shape types a **shape constraint**, a fixed size: width × height for rectangles and oriented rectangles, two radii for ellipses. Drawing that shape for that label then places it at that size instead of dragging it out. Separately, the **export footprint** (the area a polyline or points become in an export) gets a point shape (disc or square) next to the existing line width and point radius. The editor can show the footprint as a ghost, and an export can override it per label. Approved designs: boards 7–10 ("Shape constraints & export footprint") in the "oxilabel page designs" canvas, https://claude.ai/artifact/6zxSSAR75dWXyW95DYaNut.

**Blocked by:** none (builds on 08, 12, 19, 23)

**Status:** done

- [x] On the Labels page, each allowed rectangle, oriented rectangle and ellipse type has a Free/Fixed switch; Fixed takes W × H (ellipse: two radii) in original-image pixels. The label list shows the fixed sizes
- [x] Rectangle tool on a label with a fixed rectangle: a dashed ghost of the fixed size follows the cursor, centred on it, with a size readout; one click places it
- [x] Oriented rectangle and ellipse tools on a fixed label: the first click sets the centre (ghost at 0°), moving the mouse turns it with an angle readout, a second click sets the angle; Shift snaps to 15° like the rotation handle; Esc cancels. A fixed ellipse with equal radii (a circle) is placed with one click
- [x] A shape whose label fixes its size can be moved and rotated but not resized: its size handles are hidden
- [x] A constraint guides drawing only: the server does not enforce it, and imports, pre-annotations, existing annotations and relabelled shapes keep their size. Changing a constraint does not touch existing annotations
- [x] Each label's export footprint section has line width, point radius, point shape (disc or square, square side = 2 × radius) and "Show footprint in the editor" (default on)
- [x] YOLO segment and OBB export a square point footprint as a square; YOLO detect boxes polylines and points by their footprint (a point becomes a 2r × 2r box), switchable off in its export dialog (default on)
- [x] The export dialog lists the labels with polylines or points, pre-filled with their line width, point radius and (except detect) point shape. Edits apply to that export only, are highlighted, and can be reset to the label's values
- [x] The editor header has a Footprint toggle (saved in the user's own settings, default on) that shows or hides the ghosts of all labels that show a footprint: a translucent fill plus an outline in the label's colour, drawn as smooth vector shapes (a round-capped stroke of the line width, discs or squares)
- [x] Project archives and the CLI carry the new label fields; CVAT import uses the defaults (no constraints, disc points, footprint shown)
- [x] `CONTEXT.md` defines **Shape constraint** and **Export footprint** (done while planning)

**Decisions (2026-10-18):** grilled with the author, then the designs were approved.
- "Shape constraint" means fixed size only; the polyline/point area is the "export footprint". The ghost is the editor's view of the footprint.
- Constraints are optional for each shape type a label allows, and cover the rectangle, oriented rectangle and ellipse (two radii). Drawing them can't be bypassed; the label has to be edited instead.
- A constraint is a drawing aid, not validation (see the checklist).
- Viewer and exporter are separate: the label holds the footprint defaults, and an export may override them per label, for that export only (not remembered).
- The ghost is switched per label (project-wide checkbox) and in the view (one per-user toggle; the author asked for a toggle rather than an Overlays popover).
- The ghost is a vector approximation and may differ from the traced export polygon by about a pixel.
- In the design, Ctrl snaps the angle; this ticket uses Shift to match the existing rotation handle (ticket 23).

**Implementation note:** `oxilabel-core/src/constraint.rs` holds `ShapeConstraints` (`rectangle` and `oriented_rectangle` as `Size`, `ellipse` as `Radii`) with `place`, which centres the shape on the click, turns it and moves it just inside the image (`Shape::moved_inside`). `Footprint` (line width, point radius, point shape) is flattened into `Label`, so the JSON keeps `line_width` and `point_radius` at the top level and old archives load. `line_reach` and `point_reach` are the pixel rounding that both exporters and the editor's ghost use. Migration `0015_label_shapes.sql` adds `point_shape`, `show_footprint` and `constraints`. A square point paints the pixels whose centres lie in the square (`Bitmap::paint_square`). YOLO detect boxes polylines and points by the union of their traced footprint (`Detail::Exact`), unless `ExportRequest::footprint_bounds` is off. `ExportRequest::footprints` replaces label footprints for one export, and `ExportFormat::boxes` tells the dialog to offer the box option and hide the point shape. In the editor, the fixed-size tool shows a dashed ghost at the pointer; turnable shapes enter `Gesture::Placing` until a second click. Footprint ghosts are drawn first on the cleared overlay canvas, so `destination-out` cuts their inside and overlapping ghosts share one outline. The per-user toggle is `UserSettings::show_footprint`. The CLI sends no footprint overrides (labels' own footprints, boxes by footprint).
