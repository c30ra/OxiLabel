-- Jobs: batches of a project's images, each assigned to at most one user. An image is in at most
-- one job. Jobs are numbered per project, and a deleted job's number is not reused.
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    assignee_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (project_id, number)
);
CREATE INDEX jobs_by_assignee ON jobs (assignee_id);
ALTER TABLE projects ADD COLUMN next_job_number INTEGER NOT NULL DEFAULT 1;
ALTER TABLE images ADD COLUMN job_id INTEGER REFERENCES jobs (id) ON DELETE SET NULL;
CREATE INDEX images_by_job ON images (job_id);
