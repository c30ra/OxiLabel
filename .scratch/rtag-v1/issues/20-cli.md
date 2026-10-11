# 20: CLI

**What to build:** A command-line interface that talks to the same operations as the server API: import (CVAT XML, Project archive), export (every Exporter and the Project archive) and running pre-annotation headlessly on a project or job.

**Blocked by:** 06, 12, 16, 17, 18

**Status:** ready-for-agent

- [ ] I can import a CVAT XML or a Project archive into a project from the command line
- [ ] I can export any available format or a Project archive, choosing which images to include
- [ ] I can run a registered model on a project or job with a confidence threshold and see progress
- [ ] The CLI authenticates against a server and reports failures with clear messages and non-zero exit codes
- [ ] Tests cover command parsing and a run against a test server
