# 13: Enhancement filters

**What to build:** A filter panel in the viewer with presets (original, illumination flatten, CLAHE, `enhance_v2`) and live parameter sliders. A hold-key temporarily shows the original, settings are remembered per project, and annotations always stay in original image coordinates. The server computes filters and caches a separate tile pyramid per filter and parameter set.

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] Choosing a preset or moving a slider updates the visible image
- [ ] Holding the toggle key shows the original, releasing it returns to the filtered view
- [ ] Filter settings persist per project
- [ ] Filtered views are cached, and switching back to a seen setting is fast
- [ ] Filter output matches the reference implementations in the author's `defect_detection` project on test images, within a small tolerance
- [ ] Drawing on a filtered view still stores coordinates in the original image space
- [ ] Edge snapping, fuzzy fill and the edge-limited brush (tickets 21, 22) each get a switch to analyse the active filter's view instead of the original, using lossless tiles of the filtered pyramid (ADR 0002)
