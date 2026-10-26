# 08: Pre-annotation run step behind the worker port

**What to build:** A pre-annotation run is a module that, for each image, asks the worker port for predictions and turns them into pre-annotations or skip reasons: the confidence threshold, the model's class map, matching labels by name, and each label's shape check. It then stores them through the Annotation store and updates the run's progress, stopping cleanly between images when cancelled. It can be driven in-process with the fake worker, so its tests need no sockets, background polling or sleeps.

**Blocked by:** 01, 07

**Status:** ready-for-agent

- [x] The per-image step is reachable without HTTP and without spawning a task
- [x] Tests with the fake worker cover: below threshold dropped, class mapped to a label, unknown class skipped with its reason, shape refused by its label skipped, image removed during the run, cancel between images
- [x] The timing-based pre-annotation HTTP tests are replaced; one HTTP test still runs a whole run end to end
- [x] Runs started from the UI behave as before

## Comments

- Implemented in `crates/oxilabel-server/src/pre_annotation_run.rs`: `PreAnnotationRun { pool, root, project, model, labels, threshold }` with `step(&worker, image, &cancelled, &mut RunStatus) -> Result<Step, String>` (one image: predict, threshold, class map, label by name, write through `annotation_store::write(Replacing::Pending)`, whose skips — label shape check, image bounds — are merged into the status) and `run(&worker, &images, &cancelled, status, publish) -> RunStatus` (cancel between images, final state). `inference::start_run` only builds it and spawns `run`, publishing into `Runs::set`. Unit tests drive it with `FakeWorker` in process (removal mid-run via a wrapping port). HTTP: removed the cancel, removed-image, failing-worker and threshold tests (sleep/delay based or covered by the unit tests); the end-to-end run test also checks statistics now. Wire/API unchanged, so the UI is unaffected.
