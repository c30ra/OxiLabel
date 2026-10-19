# 13: Enhancement filters

**What to build:** A filter panel in the viewer with presets (original, illumination flatten, CLAHE, `enhance_v2`) and live parameter sliders. A hold-key temporarily shows the original, settings are remembered per project, and annotations always stay in original image coordinates. The server computes filters and caches a separate tile pyramid per filter and parameter set.

**Blocked by:** 04

**Status:** done

- [x] Choosing a preset or moving a slider updates the visible image
- [x] Holding the toggle key shows the original, releasing it returns to the filtered view
- [x] Filter settings persist per project
- [x] Filtered views are cached, and switching back to a seen setting is fast
- [x] Filter output matches the reference implementations in the author's `defect_detection` project on test images, within a small tolerance
- [x] Drawing on a filtered view still stores coordinates in the original image space
- [x] Edge snapping, fuzzy fill and the edge-limited brush (tickets 21, 22) each get a switch to analyse the active filter's view instead of the original, using lossless tiles of the filtered pyramid (ADR 0002)

**Decisions (2026-10-17):** settings are remembered per project in this browser (local storage), not on the server. Sliders apply live, 300 ms after they stop moving. Brightness and contrast are applied in the browser on top of any preset (analysis ignores them). The CLAHE preset is `enhance_for_tagging` (flatten, gray, CLAHE) with its parameters as sliders. The panel starts collapsed.

**Implementation note:** `oxilabel-core/src/filter.rs` names a preset and its parameters (`Filter::key`, e.g. `clahe_0.05_3.0_16`, used in `?filter=` on the tile and analysis routes and as the cache folder). `oxilabel-server/src/enhance.rs` ports `common.py` with OpenCV's kernel sizes, borders and rounding (including CLAHE's padding quirk when only one side divides by the tiles); `tests/filters.rs` compares with `common.py` output on a synthetic photo (`tests/fixtures/filters/reference.py`): mean difference ≤ 0.02 gray levels; on a real 7008 × 4672 photo `enhance_v2` and CLAHE match exactly. Very wide blurs run on a shrunk copy (about 1 s per filter on that photo, against minutes in OpenCV). numpy's float32 mean makes the script's `flatten_illumination` a few levels darker on large photos; OxiLabel's mean is exact. Each image keeps the pyramids of its 12 most recently used filters. The viewer keeps the previous filter's tiles on screen until the new ones arrive; hold O for the original. Edge snapping, the edge-limited brush and the fuzzy fill each have a "Filtered view" switch in the tool settings bar.

**Later (2026-10-17):** the `enhance_v2` preset is shown as LCN (local contrast normalisation), key prefix `lcn`.
