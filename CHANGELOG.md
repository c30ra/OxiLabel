# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## v0.4.0 - 2026-11-03
#### Features
- (**import**) import single images through the desktop or browser file dialog, replacing the image-root folder browser - (38a2b01) - c30ra
- (**projects**) deleting a project can delete its stored copies too; originals always stay, shared copies are kept and reported - (626e4a5) - c30ra
- (**users**) delete users from the Users page - (7b50cab) - c30ra
#### Bug Fixes
- (**projects**) show the full deletion report and every count, recheck shared copies after the project is gone, prune only project and archive upload folders - (9f3272a) - c30ra
- (**ui-tests**) keep the user-deletion browser steps in the user admin flow - (f4ab9d7) - c30ra
#### Miscellaneous Chores
- (**agents**) check committed trees in CI and a pre-commit hook, add review standards, rename the glossary to GLOSSARY.md - (9c665a9) - c30ra

- - -

## v0.3.0 - 2026-11-02
#### Features
- (**editor**) switch tools and relabel through one session rule - (905d42a) - c30ra
#### Bug Fixes
- (**editor**) route pie Tag and F-sizing tool choice through the shared entry-point rules - (16e80e5) - c30ra
- (**editor**) one session rule for Escape, tool keys, drags and tags - (fa53086) - c30ra
- (**editor**) keep fuzzy fill when the eraser is chosen and cover the rail - (81be435) - c30ra
#### Documentation
- record the final review fixes on the tools and relabelling ticket - (eadd733) - c30ra
- mark the brush and fill ticket done with implementation notes - (57577e4) - c30ra
- mark tools and relabelling ticket done with implementation notes - (d73a2b8) - c30ra
- mark the drawing-tools ticket done with implementation notes - (08927a7) - c30ra
#### Refactoring
- (**editor**) one label lookup and honest brush, preview and pie names - (b67aaf1) - c30ra
- (**editor**) move the brush, its sizing and the fuzzy fill into the editor session - (3d054a7) - c30ra
- forward the drawing tools' events to the editor session - (0c3c884) - c30ra
- move tools and drawing gestures into the editor session - (3dfe9be) - c30ra
#### Miscellaneous Chores
- (**merge**) final code-review fixes - (59e1236) - c30ra
- (**merge**) code-review fixes - (22f5f9e) - c30ra
- (**merge**) ticket 13 brush and fill in the editor session - (f9cfce8) - c30ra
- (**merge**) ticket 13 brush and fill in the editor session - (bd38ca8) - c30ra
- (**merge**) ticket 14 one rule for tools and relabel - (b6067c7) - c30ra
- (**merge**) the integration tip into 14 - (620dfad) - c30ra
- (**merge**) ticket 12 drawing tools in the editor session - (0f61b09) - c30ra
- merge the tools and drawing-session commit into 14 - (2736bc3) - c30ra

- - -

## v0.2.0 - 2026-10-26
#### Features
- (**ui**) limit the image grid to admins - (00ed2c7) - c30ra
- plan CVAT imports before writing - (bd7ce1e) - c30ra
- auto-mark annotated, persisted tool settings, shared image filter (tickets 26-28) - (b010b86) - c30ra
#### Bug Fixes
- (**ui**) a merged run without own edits leaves the image saved - (af597bb) - c30ra
#### Documentation
- (**worker**) implementation note for ticket 07 - (5a5261d) - c30ra
- ticket 09 implementation note - (c75c373) - c30ra
- decide ticket 14 brush mode reset - (0d21d30) - c30ra
- architecture-deepening tickets - (3b02dd8) - c30ra
#### Tests
- (**ui**) wait for the next image before going back - (32ef451) - c30ra
- follow the narrowed-label and admin-only grid rules in the browser flows - (3bd42fe) - c30ra
#### Refactoring
- (**core**) export footprint region computed in core, drawn by the editor ghost - (17770c1) - c30ra
- (**editor**) move the Select tool's gestures into a core editor session (ticket 11) - (7a1280d) - c30ra
- (**server**) one access scope for projects, jobs and images; other jobs' images are not found - (ce8375f) - c30ra
- (**server**) pre-annotation run step behind the worker port - (034880f) - c30ra
- (**server**) CVAT import, archive import and label edits write through the annotation store - (dd2c65a) - c30ra
- (**server**) project read model for exports and archives; label and annotation columns listed once - (f45c6ef) - c30ra
- (**server**) annotation store owns editor saves and pre-annotation writes - (d94f301) - c30ra
- (**stroke**) move mask stroke logic into core - (b6235ab) - c30ra
- (**ui**) move autosave rules into a pure policy in core - (a886fe7) - c30ra
- (**worker**) talk to the pre-annotation worker through a typed port with one shared fake - (3d06e4f) - c30ra
#### Miscellaneous Chores
- (**docs**) tick browser checkbox for ticket 11 - (1daf6e9) - c30ra
- (**merge**) ticket 05 access-scope - (8d447ee) - c30ra
- (**merge**) ticket 06 image-grid-admin-only - (de25de6) - c30ra
- (**merge**) ticket 04 cvat-import-planner - (11066a8) - c30ra
- (**merge**) ticket 11 editor-session-select-gestures - (82ca5b4) - c30ra
- (**merge**) integration tip into ad/05-access-scope - (108c24b) - c30ra
- (**merge**) integration tip into ad/04-cvat-import-planner - (98c332e) - c30ra
- (**merge**) integration tip into ad/11-editor-session-select-gestures - (bacf214) - c30ra
- (**merge**) integration tip into ad/06-image-grid-admin-only - (e2bfe51) - c30ra
- (**merge**) integration tip into ad/11-editor-session-select-gestures - (e895db5) - c30ra
- (**merge**) ticket 15 autosave-policy - (f8f3fc8) - c30ra
- (**merge**) ticket 09 export-footprint-region-in-core - (9a0edb6) - c30ra
- (**merge**) ticket 02 remaining-writers-use-annotation-store - (28d1dab) - c30ra
- (**merge**) integration tip into ad/02-remaining-writers-use-annotation-store - (4647e5d) - c30ra
- (**merge**) ticket 08 pre-annotation-run-step - (2eac0dd) - c30ra
- (**merge**) integration tip into ad/02-remaining-writers-use-annotation-store - (056d408) - c30ra
- (**merge**) ticket 03 project-read-model - (9e9daa9) - c30ra
- (**merge**) integration tip into ad/09-export-footprint-region-in-core - (4bdc1f5) - c30ra
- (**merge**) integration tip into ad/02-remaining-writers-use-annotation-store - (6a3c6fc) - c30ra
- (**merge**) integration tip into ad/03-project-read-model - (e750aef) - c30ra
- (**merge**) ticket 07 typed-worker-port - (d068a4b) - c30ra
- (**merge**) integration tip into ad/15-autosave-policy - (a15d89f) - c30ra
- (**merge**) integration tip into ad/07-typed-worker-port - (de519b6) - c30ra
- (**merge**) ticket 10 mask-stroke-in-core - (60aa680) - c30ra
- (**merge**) ticket 01 annotation-store - (35fb15e) - c30ra
- tidy the merged tree - (52d4416) - c30ra
- add Cocogitto release config and mise tasks - (edc8767) - c30ra

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).