# 18: Project archive

**What to build:** I can export a whole Project as a Project archive (versioned zip with labels, attributes, jobs, annotations in original coordinates, image references with hashes, and optionally the images) and import it into another rtag instance without loss.

**Blocked by:** 08, 09, 10

**Status:** ready-for-agent

- [ ] Exporting produces an archive with a format version and the option to include images
- [ ] Importing an archive recreates the project, labels (with the same permanent ids), jobs and annotations
- [ ] Export followed by import is lossless for all supported shapes, tags and attributes
- [ ] Importing an archive from a newer unsupported version fails with a clear message
- [ ] Formats tests cover round-trips for every shape type
