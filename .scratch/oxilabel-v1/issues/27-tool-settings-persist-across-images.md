# 27: Tool settings and the last tool survive moving to another image

**What to build:** The drawing tools' settings (rectangle square/centre/oriented, line snap, freehand and spacing, single point, ellipse mode, brush size, eraser, fuzzy fill threshold and options, edge snapping, stop at edges…) and the "last tool" that a quick tap of Q repeats are kept when moving to another image. Today they live in the per-image `Editor` (`crates/oxilabel-ui/src/annotate.rs`), which the workspace rebuilds for every image, so they reset to defaults and the Q tap forgets the last tool.

**Blocked by:** none (builds on 07, 11)

**Status:** done

- [x] Changing a tool's options, then opening the next image, keeps them
- [x] A quick tap of Q on the next image brings back the last tool with the same label and modes (`Editor::last`)
- [x] Browser test: set options and a last tool, move with J, check both are kept

**Open questions (answered, see Comments):**
- Keep only within the session (like the copy/paste clipboard, ticket 25), or save per user (user settings, ticket 19) so they survive a reload?
- Per project or global? The last tool's label is a project label id.
- Does the active tool itself stay selected on the next image (e.g. stay on Brush), or only the options and the Q memory?
- Does the current label carry over too?
- Which settings are excluded (e.g. the view filter's analysis switches, the pre-annotation threshold)?

## Comments

Decided with the author:
- Kept for the session, per project: the options and the last tool (`Tools` in `annotate.rs`) are made by the workspace page, which stays while images of a project are opened, and each image's `Editor` uses them. A reload starts from the defaults again.
- The next image opens with Select and the project's first label; only the options and the Q memory carry over.
- Included: every drawing tool option and the analyse-the-filtered-view switches. Not included: the pre-annotation threshold.

Changed after the author found the options and the Q memory lost when going back to the grid and opening another image: `Tools::for_project` keeps them in session storage per project, so they survive leaving the editor and reloading the page (still not across browser tabs or sessions).
