# 06: The image grid is for admins only

**What to build:** Only admins see a project's image grid. Annotators work through their jobs: opening a project takes them to the Jobs tab, the Images tab is not shown to them, and going to the grid's address directly shows a short "Only admins browse all images" note with a link to Jobs, the way the Models and Users pages do. In the editor an annotator still moves through the images of their job with the filmstrip and j/k, and Ctrl+Enter still marks and opens the next one.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] An annotator opening a project lands on Jobs; the Images tab is hidden for them
- [x] An annotator going to the grid's address sees the note, not the grid
- [x] Admins see the grid exactly as before
- [x] Browser test: sign in as an annotator, open a project, open a job's first image, move with j and k within the job

## Comments

- `pages/project.rs`: the Images tab link is admin-only; `Tab::Images` for an annotator renders "Only admins browse all images." with a link to the Jobs tab (like Models/Users), and the image-count probe is skipped for them. `pages/projects.rs`: an annotator's project card links to `/projects/{id}/jobs`. The editor (filmstrip, j/k, Ctrl+Enter) is untouched; the server already scopes an annotator's image ids to their jobs. Browser test: the existing "assigned jobs" test now walks the annotator through card → Jobs → the refused grid → the job's first image → j/k.
