# 26: Mark an image annotated as soon as it has an annotation

**What to build:** An image becomes annotated as soon as an annotation or tag is added to it in the editor, without pressing Ctrl+Enter or the "Mark annotated" button. Today the `annotated` flag is only set by hand (the image status endpoint in `crates/oxilabel-server/src/images.rs`).

**Blocked by:** none (builds on 11)

**Status:** done

- [x] Adding the first annotation or tag to an image (drawn, pasted, accepted pre-annotation…) marks it annotated; the grid and filmstrip dot update
- [x] Saving an image left with no reviewed annotation or tag unmarks it
- [x] Browser test: draw a shape on an unannotated image and see it marked, undo it and see it unmarked

**Open questions (answered, see Comments):**
- Server side (any save with at least one annotation sets the flag) or editor side (the UI calls the mark endpoint)? Server side would also cover the CLI and imports.
- Do pending pre-annotations count, or only manual work and reviewed ones?
- Does removing the last annotation unmark the image, or does "annotated" stay set once reached?
- Do imports (CVAT, archives) and pre-annotation runs mark images too?
- Does Ctrl+Enter keep its meaning (mark and open the next) for images with no annotations, e.g. images confirmed empty?

## Comments

Decided with the author:
- Server side: every annotation save sets `images.annotated` to whether the saved set has an annotation or tag that is not a pending pre-annotation (`counts_as_annotated` in core), so the editor, paste, accepted pre-annotations and the CLI all mark images. The load and save responses carry `annotated`, and the editor shows it.
- Pending pre-annotations do not count.
- A save that leaves no reviewed annotation unmarks the image. Only saves change the status: imports, pre-annotation runs and deleting a label leave it as it is.
- The status button and Ctrl+Enter keep working by hand, e.g. for images confirmed empty (until their next save).
