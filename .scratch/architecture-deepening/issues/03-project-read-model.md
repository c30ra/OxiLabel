# 03: Project read model for exporters and project archives

**What to build:** Exports and project archive downloads read a project through one module: the project, its labels, and its images in path order, each with its annotations and its resolved source file, with a choice of including pending pre-annotations or not. Exporters map it to their export input; project archives map it to the archive. The column lists for label and annotation rows are owned once, beside the code that turns rows into labels and annotations, so adding a label or annotation field is one change. Image source files are resolved the same way for both (the archive's canonical, never-outside-the-root rule).

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Exports and project archives no longer contain their own project, label, image or annotation queries
- [ ] Label and annotation column lists appear once each
- [ ] An export and an archive of the same project resolve every image to the same file
- [ ] Read model tests on in-memory SQLite cover image order, annotation order within an image, the pending filter, and images without annotations
- [ ] Existing export and archive HTTP tests and the exporter tests still pass
