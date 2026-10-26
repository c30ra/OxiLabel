# 05: Access scope

**What to build:** One server module, built from the signed-in user, owns which projects, jobs and images that user can see. Handlers ask it for a visible project, a visible image (with its facts and its resolved source file), and image queries already narrowed to the user's jobs; they no longer know that job assignment exists. Anything an annotator cannot see is not found (404), for images exactly as for projects, so an annotator cannot tell another job's image ids from ids that don't exist. Starting a pre-annotation run checks all chosen images at once instead of two queries per image. Stored image paths (relative to the image root, absolute, or uploaded copies, ADR 0003) are turned into files in this one place.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

**Decision:** an image outside the annotator's jobs gives 404, like a project. Images are split by jobs, so in normal use an annotator never asks for one.

- [x] Tiles, analysis tiles, thumbnails, annotations, image details, status, neighbours, image ids, pre-annotation runs and job lists all go through the access scope
- [x] No handler compares a job's assignee itself or splices the job filter into SQL
- [x] An annotator asking for an image in another job gets 404 on every image route
- [x] One access-matrix test on in-memory SQLite covers admin, assigned annotator and unassigned annotator against every entry point
- [x] Existing jobs, auth and pre-annotation HTTP tests pass (updated from 403 to 404 where they asserted it)

## Comments

`crates/oxilabel-server/src/access_scope.rs` is the one place that knows job assignment bounds
access. `AccessScope` (a `FromRequestParts` extractor, like `CurrentUser`) exposes:

- `sql()` — a `QueryBuilder` opening with the inlined views `visible_projects`, `visible_jobs`
  and `visible_images` (the rows the user may see; admins see everything, jobless images
  included). Image and job listings, the project list and the project detail query these views
  instead of splicing an assignee condition; `push_filter` in `images.rs` no longer takes an
  owner.
- `project(conn, project)` — `NotFound` unless visible.
- `project_image(conn, project, image)` / `image(conn, image)` — `VisibleImage { image, hash,
  source }`, or `NotFound` for an image of another project or another user's job (no longer
  `Forbidden`). Tiles, analysis tiles, thumbnails, annotations, image details, status and
  neighbours all take it.
- `job_images(conn, project, job)` — a job's images, `NotFound` for a job the user cannot see.
- `check_run_images(conn, project, &ids)` — one `json_each` query for a whole run instead of two
  queries per image; `start_run` calls it once, and `run_status`/`cancel_run` check the run's
  project through `project`.

The owner rule (`annotator ⇒ own id, admin ⇒ none`) moved out of `auth::CurrentUser` into
`AccessScope::job_owner`, so nothing else compares roles for access. Paths become files only in
`images::source_file(root, stored)` (ADR 0003), shared by `AccessScope`, the project read model
and `pre_annotation_run`, which previously joined the root itself.

`tests/access_scope_api.rs` is the matrix: one in-memory app with two images (one in an
annotator's job, one in an unassigned job and its own jobless work), run as admin, as the
assigned annotator and as an annotator with no job, against every image and project entry point.
`tests/jobs_api.rs` and `tests/pre_annotations_api.rs` now expect 404 where they asserted 403.