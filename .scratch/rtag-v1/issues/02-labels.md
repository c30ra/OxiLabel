# 02: Labels

**What to build:** Within a Project I can define Labels with a permanent numeric id, name, color, hotkey, allowed shape types and an ignore flag. Renaming and recoloring never affect ids or existing annotations, and deleting a label that is in use is guarded.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] I can add, edit and remove labels in a project's label screen
- [ ] Each label gets a numeric id that is never reused or renumbered, even after deleting other labels
- [ ] A label can be restricted to certain shape types and flagged as an ignore label
- [ ] Deleting a label that is in use tells me what would be removed and requires confirmation
- [ ] API tests cover id stability across deletes and renames
