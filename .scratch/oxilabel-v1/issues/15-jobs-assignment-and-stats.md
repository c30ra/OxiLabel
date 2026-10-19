# 15: Jobs, assignment and statistics

**What to build:** Admins split a Project's images into Jobs and assign each to an annotator. Annotators see only their jobs, own them, and a Job's owner is enforced when saving. A statistics view shows annotation counts per label, job and user.

**Blocked by:** 03, 14

**Status:** done

- [x] I can create a job from selected images or a filter and assign it to an annotator
- [x] An annotator sees their assigned jobs and can only edit images in jobs they own
- [x] Admins can see and edit all jobs and reassign them
- [x] The statistics screen shows annotation counts per label, per job and per user
- [x] API tests cover job ownership enforcement on annotation saves

**Decisions (2026-10-17):** an image is in at most one job; a new job made from images already in jobs moves them (emptied jobs stay, so their numbers keep meaning the same). Annotators see only their jobs: the projects with jobs of theirs (counts cover only those jobs), only those jobs' images in the grid, filmstrip and next/previous, and they open, annotate and set the status of only those images. Statistics count "per user" by the job's current assignee, so reassigning a job moves its counts (annotations do not record who drew them). "Create jobs…" on the grid's selection (Select all takes everything the filter matches) makes one job or splits the selection into jobs of N images in path order, optionally assigned.

**Implementation note:** `oxilabel-server/src/jobs.rs`: `GET/POST /api/projects/{p}/jobs`, `PUT/DELETE /api/projects/{p}/jobs/{id}` (assign or unassign; deleting leaves the images in no job), `GET /api/projects/{p}/statistics` (admin only). Jobs are numbered per project (`projects.next_job_number`, never reused) and shown as "Job N". Access lives in two helpers there: `ensure_project_visible` (404 for an annotator without a job in the project) and `ensure_image_access` (403 for an image outside their jobs), used by projects, labels, images, annotations, tiles and thumbnails; image lists add the restriction in `push_filter`. `ImageFilter` gained `job`, `without_job` and `annotated` (no grid control for the last yet); the grid has a Job menu next to Filters. The project page has Jobs and (for admins) Statistics tabs; a job's Annotate button opens its first image not yet marked annotated. Job creation passes the selected ids as one JSON array (`json_each`), so Select all on a big project stays under SQLite's bind limit. For admins the workspace's next/previous still walks the whole project, not the job they opened. Project cards show the number of jobs.
