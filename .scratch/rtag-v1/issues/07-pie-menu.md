# 07: Pie menu

**What to build:** Holding Q or the right mouse button opens a two-layer pie menu at the cursor: ring 1 picks the tool, ring 2 picks the label, and flicking selects. Once a tool is active, the same key opens that tool's options ring. This ticket builds the menu mechanics using the rectangle tool, so later tools just plug in.

**Blocked by:** 05

**Status:** done

- [x] Hold-and-flick selects a tool, then a label, and enters editing mode with that tool
- [x] A quick tap of the pie key repeats the last tool and label
- [x] In editing mode the key opens the tool options ring (rectangle: 2-point vs oriented placeholder), and Esc discards a shape in progress, then returns to tool-choose mode
- [x] The label ring shows about 8 slices with pinned and recent labels first, plus a "more" slice or type-to-search for the rest
- [x] Number hotkeys select labels directly, and a label-ring key opens only the label ring
- [x] A selected shape's label can be changed from the pie menu
- [x] Menu geometry and slice selection logic are unit-tested in the core crate, and canvas interaction tests cover the main flow

**Implementation note (built together with ticket 05 after review):** `rtag_core::pie` holds the geometry (compass slots, nearest-slot pick with an 18 px dead zone, flick at 150 px, tap under 220 ms), unit-tested. The UI (`pie.rs`, wired in `annotate.rs`) follows the approved design: rounded buttons at compass positions, a ring whose arc follows the pointer. Hold Q or the right mouse button: with Select active and nothing selected the tool pie opens (later tools are shown disabled in their final places); picking a tool opens the label pie centred at the pointer (still held if Q or the right button is down); release, flick or a click in a direction picks in every pie; a tap repeats the last tool and label (or leaves the pie open for clicking). With a drawing tool active, Q opens its options pie (rectangle: 2-point, 3-point oriented placeholder, From center and Square toggles that Alt and Shift invert). With a shape selected, Q opens the label pie and relabels it. Shift+Q is the label-ring key. While a pie is open, item keys (tool letters, label hotkeys, "/" for More…) pick and Esc closes. The label pie shows 7 labels in label-id order (stable positions for muscle memory, rather than recent-first) plus "More…" with type-to-search when there are more than 8; labels that don't allow the shape stay in place, disabled. Browser flows live in `scripts/ui-tests` (run with `scripts/ui-test.sh`).


**Change (2026-10-12, user request):** a quick tap of Q or the right button repeats the last drawing tool with its label and modes (e.g. Polyline freehand) from any tool, not only from Select; a drawing tool's options pie opens when the key is held. Options pies list both modes of each choice with the current one marked, and pie buttons grow with their text away from the centre.
