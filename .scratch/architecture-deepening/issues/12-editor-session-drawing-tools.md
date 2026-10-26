# 12: Editor session: drawing tools

**What to build:** The drawing tools run in the editor session from 11: rectangle (two clicks or a drag, square, from the centre, 3-point oriented), polygon, polyline and points (click per vertex, Enter, double-click, closing on the first vertex, Backspace, 15° snap), freehand polylines with spacing and edge snapping, single points, ellipse (centre or box, circle), and placing a fixed-size shape from a label's shape constraint. Each tool's press, move, release, finish, preview and cancel live together, and Escape's order (cancel the shape, then clear the selection, then back to Select) is the session's rule.

**Blocked by:** 11

**Status:** ready-for-agent

- [ ] Every drawing tool runs through the session; the annotation layer only forwards events and draws the session's preview
- [ ] Shapes too small to be intended are still not created
- [ ] Rust tests per tool cover drawing, finishing, cancelling, modifiers and new annotations getting their label's default attribute values
- [ ] Browser drawing tests still pass
