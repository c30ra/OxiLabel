# 28: Find images sharing a file name, filter by label, and one filter for the grid and the editor

**What to build:** In the image grid, find images whose file name (the last part of the path) another image of the project has too, compare them and remove the ones not wanted. Filter images by the labels of their annotations and tags. The grid's filter is kept for the browser tab (surviving a reload) and is the same filter the editor moves through.

**Blocked by:** none (builds on 03, 11)

**Status:** done

- [x] Filter "File name: shared with another image" lists only namesakes, grouped by name (exact, case-sensitive base name; byte-identical copies are already skipped at import)
- [x] In that view each tile shows path, dimensions, file size, creation date (where the file system records it) and modification date; clicking the thumbnail opens a zoomable preview instead of the editor
- [x] Removing images says how many annotations are discarded with them (dry run of the removal)
- [x] "Has label" filter, one checkbox per label of the project; an image matches with an annotation or tag of any checked label
- [x] The filter and the grid page are kept in session storage per project
- [x] The editor's J/K, gg/G, filmstrip and position follow the filter and its order; an "Images" chip in the editor header edits the same filter
- [x] API and browser tests

## Comments

Decided with the author: names only (no perceptual near-duplicate search), a filter in the image grid rather than its own screen, and a removal discards the image's annotations after a warning.

Implementation: `images.created_at` (migration 0016) is recorded at import and backfilled at start-up; files whose file system has no creation time are looked at again on each start. The filter lives in `crates/oxilabel-ui/src/image_filter.rs`. An open image the filter leaves out shows "– / N" and J/K go to the matching images on either side of it.
