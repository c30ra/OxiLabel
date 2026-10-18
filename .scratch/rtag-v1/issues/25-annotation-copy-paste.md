# 25: Copy and paste annotations

**What to build:** In the editor, Copy (Ctrl+C) puts the selected annotations on an in-app clipboard, Cut (Ctrl+X) copies them and deletes the unlocked ones, and Paste (Ctrl+V) adds copies to the open image at the same image coordinates, so the same object can be carried across images (video frames) of one project. The copies are fresh manual work, selected after pasting so a drag moves them.

**Blocked by:** none (builds on 11, 19, 24)

**Status:** done

- [x] Copy, Cut and Paste are keymap actions in the Editing group (Ctrl+C, Ctrl+X, Ctrl+V), rebindable in Settings; in text fields the keys keep their native behaviour
- [x] Copy takes the selected annotations, locked ones included; with nothing selected it does nothing
- [x] Cut copies the selection, then removes its unlocked annotations in one undo step (like Delete)
- [x] The clipboard lives in the app's memory for the project: it survives moving to another image, not a reload or another tab; Paste with an empty clipboard does nothing
- [x] Paste adds the copies at the same image coordinates in one undo step, cancels a shape being drawn, switches to Select and selects the copies
- [x] A copy keeps shape, label and attribute values; it has no source or confidence and is not pending, locked or hidden. A fixed-size constraint (ticket 24) does not resize it
- [x] Image tags are never copied
- [x] A pasted mask is its own annotation, never merged with touching masks of its label
- [x] On an image of another size, a copy sticking out is moved just inside it (`Shape::moved_inside`)
- [x] A copy is skipped when it is bigger than the image, its label was deleted, or its label no longer allows its shape type; the rest are pasted, and one notice (`editor.notice`) says how many were skipped and why
- [x] Browser tests: copy on one image and paste on another, cut, and a skipped copy with its notice

**Decisions (2026-10-18):** grilled with the author.
- Copy/paste acts on annotations in the editor, not on label definitions (projects already copy labels at creation).
- Same project only: label ids are per project.
- Same place rather than under the pointer, matching CVAT; no ghost placement, no Duplicate (Shift+D), no pie entries, no "paste from previous image".
- In-app clipboard rather than the system one: no browser permission prompts.
- The server refuses shapes outside the image or with unknown labels (which would block the image's autosave), so such copies are skipped with a warning instead of clamped or relabelled.

**Implementation note:** `rtag-core/src/clipboard.rs` holds the rules: `copied` (fresh manual copies, no tags) and `paste` (moves copies inside the image, skips the three cases, counts them for `Pasted::notice`). Pasted attribute values are also made to fit the label as it is now (`conform_attributes`), as loading does, so a label edited since the copy cannot make the save fail. The clipboard is a thread-local in `annotate.rs` tagged with the project id, since the workspace (and its `Editor`) is rebuilt for each image. Cut removes only what it copied: selected tags stay. Keymap actions `copy`, `cut`, `paste`.
