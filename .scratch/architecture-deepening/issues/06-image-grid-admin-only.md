# 06: The image grid is for admins only

**What to build:** Only admins see a project's image grid. Annotators work through their jobs: opening a project takes them to the Jobs tab, the Images tab is not shown to them, and going to the grid's address directly shows a short "Only admins browse all images" note with a link to Jobs, the way the Models and Users pages do. In the editor an annotator still moves through the images of their job with the filmstrip and j/k, and Ctrl+Enter still marks and opens the next one.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] An annotator opening a project lands on Jobs; the Images tab is hidden for them
- [ ] An annotator going to the grid's address sees the note, not the grid
- [ ] Admins see the grid exactly as before
- [ ] Browser test: sign in as an annotator, open a project, open a job's first image, move with j and k within the job
