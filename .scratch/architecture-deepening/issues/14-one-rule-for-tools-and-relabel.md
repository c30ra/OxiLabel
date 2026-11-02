# 14: One rule for switching tools and relabelling

**What to build:** Choosing a tool works the same from a key, the tool pie, the tool rail and a quick tap of Q: the same cancel of the gesture in progress, the same reset or keep of the brush's eraser and fill modes, and the same choice of a label that allows the tool's shape. Relabelling selected annotations works the same from the label pie and the side panel: tags stay one per label, attributes are conformed to the new label, pending pre-annotations count as reviewed, and hidden annotations are left alone. Both rules live in the editor session.

**Blocked by:** 11

**Status:** done

**Decision:** choosing Brush resets to plain painting from every entry point (key, pie, rail, Q tap), as the B key does today; the last eraser or fuzzy fill mode is not kept.

- [x] Keys, pie, rail and tap all switch tools through one session rule
- [x] The side panel and the label pie relabel through one session rule; the side panel's duplicate-tag gap is closed
- [x] Rust tests cover each entry point giving the same tool, modes and label, and relabelling with a duplicate tag, a hidden annotation and a pending pre-annotation
- [x] Browser pie and side panel relabel tests still pass

## Comments

Implemented in the core editor session, with the annotation layer only forwarding its own brush state:

- `crates/oxilabel-core/src/editor/mod.rs`: `Tool` (moved by ticket 12) plus `BrushMode` (`Paint`, `Erase`, `Fill`, `FillErase`; the eraser and fuzzy fill combine, so a fill in eraser mode cuts the region) with `with_eraser`/`with_fill`/`erases`/`fills`. `Session` gains `tool`/`label`/`brush`, `switch_tool`, `brush_mode`, `set_brush_mode`, `set_brush_eraser`, `set_brush_fill`, `can_use`, `label_allows`, `unusable_notice`, `relabel` and `relabel_selection`.
- `switch_tool(tool) -> bool` is the one switching rule: refuses a tool no label allows (notice, nothing changes), cancels the session gesture even when the same tool is chosen again, makes Brush plain painting, and keeps the label only while it allows the tool's shape (otherwise takes the first label that does). Every entry point calls it: tool keys, `E`/`U` (which set their mode afterwards), Esc's return to Select, paste, the tool pie, the tool rail and the Q-tap `Editor::restore`.
- `relabel(indices, label) -> bool` / `relabel_selection(label)` is the one relabelling rule: one undo step, skips annotations that are not shown (hidden or below the pre-annotation threshold) and shapes the label does not allow, conforms attributes to the new label, clears `pending`, and keeps one tag per label (a tag whose target label another tag already has is left alone with a notice; when several selected tags take one label, the first takes it). The label pie and the objects list both use it; the objects list's select snaps back when the session changed nothing, so a hidden row or a duplicate tag cannot leave the dropdown showing a wrong label.
- `crates/oxilabel-ui/src/annotate.rs`: `Editor::switch_tool`, `set_label`, `set_brush_mode`, `set_brush_eraser`, `set_brush_fill`, `brush_mode`, `relabel`, `relabel_selection`; a `switched` trigger lets the layer drop its brush gesture; the duplicate label-fixup effect and the `usable_shapes` signal are gone (`can_use` asks the session). `Tool` is re-exported from core.
- `crates/oxilabel-ui/src/pages/workspace.rs`: the rail and the label picker go through the editor methods.
- `crates/oxilabel-ui/src/side_panel.rs`: the objects list relabels through the session rule.

Tests:

- `crates/oxilabel-core/src/editor/tests/tools.rs`: cancellation, label keeping/choosing, refusal with a notice, each of the four entry points giving Brush + painting + the first mask-allowing label, the eraser/fill reset, and the eraser keeping fuzzy fill.
- `crates/oxilabel-core/src/editor/tests/relabel.rs`: attribute conformance and pending review in one undo step, a duplicate tag refused, two tags taking one label, hidden annotations left alone, and the side-panel single-annotation path.
- `scripts/ui-tests/tests.mjs`: a tool-rail flow that checks the rail switches tools and resets the brush to painting.
- `cargo test --workspace --exclude oxilabel-ui` passes; `scripts/ui-test.sh` passes all 70 browser flows, including the Q flick/tap, tool pie, label pie relabel, objects-list relabel and the new rail flow. One unrelated full-suite run timed out in the settings page's save check while other worktrees were building; it passed alone and in the last two full runs.

Merged `architecture-deepening` (ticket 12's drawing-tools session) before finishing; conflicts were limited to the session's tool/method blocks and the annotation layer's imports and options effects.

**Final review fixes (`16e80e5`, merged in `59e1236`):**

- Tag from the tool pie now opens the label pie like keyboard T (held while Q or the right button is down) instead of making Tag the active tool; Tag never becomes active, so the current tool and its gesture stay.
- While the brush is being sized with F, tool keys and Q are no longer swallowed: `Session::cancel` restores the size from before sizing, so a tool switch, Q and Esc all end sizing the same way (`restore_brush_size` is gone).
- The core test that claimed four entry points is now `switching_to_brush_resets_its_modes_and_chooses_a_mask_label` (the shared rule only); the browser flow `key, tool pie, rail and Q tap choose Brush with the same mode and label` exercises the real entry points, and the F and T/Tag flows cover the two fixes.
- Standards: shape-constraint vocabulary (`constrained_shape`, `constrained_preview`), one `TOOL_METADATA` table for `Tool`, `BrushMode::from_flags`, `Editor::mirror_tool_state`, `with_analysis_layer`, and one radio-group renderer in `tool_options.rs`.
- `cargo test --workspace --exclude oxilabel-ui`: 425 passed. `scripts/ui-test.sh`: 69/70 passed; the settings save check timed out again under load and passed alone.
