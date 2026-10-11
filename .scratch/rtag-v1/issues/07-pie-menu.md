# 07: Pie menu

**What to build:** Holding Q or the right mouse button opens a two-layer pie menu at the cursor: ring 1 picks the tool, ring 2 picks the label, and flicking selects. Once a tool is active, the same key opens that tool's options ring. This ticket builds the menu mechanics using the rectangle tool, so later tools just plug in.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Hold-and-flick selects a tool, then a label, and enters editing mode with that tool
- [ ] A quick tap of the pie key repeats the last tool and label
- [ ] In editing mode the key opens the tool options ring (rectangle: 2-point vs oriented placeholder), and Esc discards a shape in progress, then returns to tool-choose mode
- [ ] The label ring shows about 8 slices with pinned and recent labels first, plus a "more" slice or type-to-search for the rest
- [ ] Number hotkeys select labels directly, and a label-ring key opens only the label ring
- [ ] A selected shape's label can be changed from the pie menu
- [ ] Menu geometry and slice selection logic are unit-tested in the core crate, and canvas interaction tests cover the main flow
