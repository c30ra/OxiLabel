# 13: Editor session: brush and fuzzy fill gestures

**What to build:** Painting, erasing, resizing the brush with F, and fuzzy fill (seed, threshold drag, commit when growing completes) run through the editor session, using the core mask stroke from 10. Undo and redo during a stroke follow the session's one rule for gestures holding annotation indices, replacing the special case that only guards painting. The editor keeps the canvas sprites and the tile reading for analysis.

**Blocked by:** 10, 12

**Status:** ready-for-agent

- [ ] Brush, eraser, F resizing and fuzzy fill gestures run through the session
- [ ] The painting-only undo guard is gone; the session's rule covers it
- [ ] Rust tests cover a stroke, Ctrl inverting it, F resize and Esc restoring the size, a fill's threshold drag and a fill committed after release
- [ ] Browser brush and fuzzy fill tests still pass
