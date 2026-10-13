# 11: Objects list, filmstrip and status

**What to build:** The workspace gets a collapsible objects list (select, hide, lock, relabel), a filmstrip of neighbouring images with their status, previous and next image keys, and a toggle for the annotated or not annotated status of an image.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] The objects list shows every annotation on the image and supports select, hide, lock and relabel
- [ ] The filmstrip shows neighbouring images with their status and lets me jump to one
- [ ] Previous and next keys move through the images
- [ ] I can mark an image as annotated or not annotated, and the grid and filmstrip reflect it
- [x] Locked shapes cannot be moved or deleted until unlocked

**Implementation note (2026-10-13):** decisions asked of the user: hide and lock are saved with the annotation (exports ignore them); previous and next follow vim motions: j next, k previous, gg first, G last; Ctrl+Enter marks the image annotated and opens the next one, while the header button only toggles the status. Core: `Annotation` gains `hidden` and `locked` (left out of the JSON when false); hit-testing and box selection skip hidden shapes. Server: migration 0007 adds the columns; `PUT /api/projects/{p}/images/{i}/status` sets the status; `GET …/images/{i}/neighbours?count=N` gives the filmstrip window with the image's position. UI: the side panel always shows the collapsible objects list (shapes in drawing order, tags keep their own section): colour, a label menu to relabel (labels allowing the shape), the shape and its size or points (and source and confidence for model results), hide and lock buttons (each an undo step); clicking a row selects it, Shift adds. Hidden shapes are not drawn, picked, selected by A or the list, or edited. Locked shapes have no handles and are not moved, deleted, painted over, erased or merged into (a notice says to unlock them); they can still be relabelled and have attributes edited. The header shows "N / total" and the status button; the filmstrip shows the neighbouring images with status dots. Navigation keys are ignored while typing, with a dialog or pie open, while a shape is being drawn, and when the editor used the key (a label hotkey).
