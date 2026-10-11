# 02: Labels

**What to build:** Within a Project I can define Labels with a permanent numeric id, name, color, hotkey, allowed shape types and an ignore flag. Renaming and recoloring never affect ids or existing annotations, and deleting a label that is in use is guarded.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] I can add, edit and remove labels in a project's label screen
- [x] Each label gets a numeric id that is never reused or renumbered, even after deleting other labels
- [x] A label can be restricted to certain shape types and flagged as an ignore label
- [ ] ~~Deleting a label that is in use tells me what would be removed and requires confirmation~~ moved to ticket 05: no annotations exist before it. Deleting any label already asks for confirmation in the UI.
- [x] API tests cover id stability across deletes and renames

**Implementation note:** label ids are a per-project counter starting at 1, never reused (0 stays free for the mask background). Validation lives in `LabelDefinition::validated` in the core crate: non-blank unique name, `#rrggbb` color, single-key unique hotkey, at least one shape type. Omitted shape types mean all shapes are allowed.
