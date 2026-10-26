# 04: Pure CVAT import planner

**What to build:** A CVAT import is planned before anything is written. A pure planner takes the parsed CVAT export, the project's labels and images, the folder search results and the user's choices, and returns the import report plus the list of labels, images and annotations to create. The preview shows that plan; committing applies it in one transaction (new labels and images first, then annotations through the Annotation store). The preview no longer stands in placeholder ids for labels and images it would create, and preview and commit cannot disagree because they share the plan.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Preview and commit both come from the same plan; no placeholder ids remain
- [ ] Planner tests (no database, no multipart) cover: label matched by name ignoring case, created, or skipped; image matched by name and size, ambiguous, size mismatch, found in the chosen folder; attributes and tags filtered; skipped reasons counted
- [ ] Most CVAT HTTP tests become planner tests; one or two HTTP tests keep covering preview and commit end to end
- [ ] Browser CVAT import flow still works
