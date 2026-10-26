# 02: CVAT import, project archive import and label edits write through the Annotation store

**What to build:** Every remaining writer of annotations goes through the Annotation store from 01: CVAT import (append, or replace an image's annotations), project archive import (a new image's annotations), and label changes that touch stored annotations (conforming attribute values, deleting a label). The rules the store enforces then hold for every path: a project archive with two copies of one tag on an image is refused with a clear reason instead of stored, and narrowing a label's allowed shape types is refused while annotations of the removed shape types exist, instead of leaving images that fail their next save. The annotated status still changes only on an editor save.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] No module except the store inserts, deletes or re-positions annotation rows, or bumps an image's version for an annotation change
- [ ] Importing a project archive whose image has a duplicate tag reports the problem and stores no duplicate
- [ ] Removing a shape type from a label that still has annotations of that type is refused with a message naming how many annotations use it
- [ ] Store tests cover the append path (positions after existing annotations, tags already on the image skipped)
- [ ] Existing CVAT import, archive and label HTTP tests still pass
