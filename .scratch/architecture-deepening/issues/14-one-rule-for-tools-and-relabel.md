# 14: One rule for switching tools and relabelling

**What to build:** Choosing a tool works the same from a key, the tool pie, the tool rail and a quick tap of Q: the same cancel of the gesture in progress, the same reset or keep of the brush's eraser and fill modes, and the same choice of a label that allows the tool's shape. Relabelling selected annotations works the same from the label pie and the side panel: tags stay one per label, attributes are conformed to the new label, pending pre-annotations count as reviewed, and hidden annotations are left alone. Both rules live in the editor session.

**Blocked by:** 11

**Status:** ready-for-agent

**Open question:** after choosing Brush from the rail, should the last eraser or fuzzy fill mode be kept (as the rail does today) or reset (as the B key does)? Ask the author before building.

- [ ] Keys, pie, rail and tap all switch tools through one session rule
- [ ] The side panel and the label pie relabel through one session rule; the side panel's duplicate-tag gap is closed
- [ ] Rust tests cover each entry point giving the same tool, modes and label, and relabelling with a duplicate tag, a hidden annotation and a pending pre-annotation
- [ ] Browser pie and side panel relabel tests still pass
