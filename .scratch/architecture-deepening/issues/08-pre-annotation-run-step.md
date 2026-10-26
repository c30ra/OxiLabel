# 08: Pre-annotation run step behind the worker port

**What to build:** A pre-annotation run is a module that, for each image, asks the worker port for predictions and turns them into pre-annotations or skip reasons: the confidence threshold, the model's class map, matching labels by name, and each label's shape check. It then stores them through the Annotation store and updates the run's progress, stopping cleanly between images when cancelled. It can be driven in-process with the fake worker, so its tests need no sockets, background polling or sleeps.

**Blocked by:** 01, 07

**Status:** ready-for-agent

- [ ] The per-image step is reachable without HTTP and without spawning a task
- [ ] Tests with the fake worker cover: below threshold dropped, class mapped to a label, unknown class skipped with its reason, shape refused by its label skipped, image removed during the run, cancel between images
- [ ] The timing-based pre-annotation HTTP tests are replaced; one HTTP test still runs a whole run end to end
- [ ] Runs started from the UI behave as before
