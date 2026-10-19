# 20: CLI

**What to build:** A command-line interface that talks to the same operations as the server API: import (CVAT XML, Project archive), export (every Exporter and the Project archive) and running pre-annotation headlessly on a project or job.

**Blocked by:** 06, 12, 16, 17, 18

**Status:** done

- [x] I can import a CVAT XML or a Project archive into a project from the command line
- [x] I can export any available format or a Project archive, choosing which images to include
- [x] I can run a registered model on a project or job with a confidence threshold and see progress
- [x] The CLI authenticates against a server and reports failures with clear messages and non-zero exit codes
- [x] Tests cover command parsing and a run against a test server

**Decisions (2026-10-18):** `oxilabel login` asks for the password once (or reads it with `--password-stdin`) and keeps the session cookie in `~/.config/oxilabel/session` (`$XDG_CONFIG_HOME` respected), readable only by the user; `oxilabel logout` ends it on the server and removes it.

**Implementation note:** crate `oxilabel-cli`, binary `oxilabel`, a client of the server's HTTP API (reqwest, blocking): `login`, `logout`, `projects`, `models`, `import cvat --project P FILE [--label CVAT=LABEL|new|skip]… [--images-folder] [--replace] [--dry-run]`, `import archive FILE [--name] [--images-folder]`, `export --project P --format ID|archive --out FILE [--images all|annotated|job:N] [--include none|symlink|copy] [--split 80/20/0]`, `pre-annotate --project P --model M [--job N] [--threshold X]` (polls the run and prints progress). Projects and models are given by id or name. Runs gained an optional `threshold` on the server (predictions below it are left out and counted). Failures print `oxilabel: <reason>` and exit 1; command lines that do not parse exit 2 with clap's message. Tests: parsing in `args.rs`; `tests/cli.rs` runs a whole session (sign-in, CVAT import with a dry run, a YOLO export, an archive export and import, a pre-annotation with a threshold, failures, logout) against a real server and a fake worker on free ports.
