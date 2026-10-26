# 05: Access scope

**What to build:** One server module, built from the signed-in user, owns which projects, jobs and images that user can see. Handlers ask it for a visible project, a visible image (with its facts and its resolved source file), and image queries already narrowed to the user's jobs; they no longer know that job assignment exists. Anything an annotator cannot see is not found (404), for images exactly as for projects, so an annotator cannot tell another job's image ids from ids that don't exist. Starting a pre-annotation run checks all chosen images at once instead of two queries per image. Stored image paths (relative to the image root, absolute, or uploaded copies, ADR 0003) are turned into files in this one place.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

**Decision:** an image outside the annotator's jobs gives 404, like a project. Images are split by jobs, so in normal use an annotator never asks for one.

- [ ] Tiles, analysis tiles, thumbnails, annotations, image details, status, neighbours, image ids, pre-annotation runs and job lists all go through the access scope
- [ ] No handler compares a job's assignee itself or splices the job filter into SQL
- [ ] An annotator asking for an image in another job gets 404 on every image route
- [ ] One access-matrix test on in-memory SQLite covers admin, assigned annotator and unassigned annotator against every entry point
- [ ] Existing jobs, auth and pre-annotation HTTP tests pass (updated from 403 to 404 where they asserted it)
