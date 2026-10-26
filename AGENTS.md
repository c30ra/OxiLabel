## Agent skills

### Issue tracker

Issues are tracked as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-label vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Testing

- `cargo test --workspace --exclude oxilabel-ui`: unit tests (core), HTTP API tests (server, in-memory SQLite), and the CLI (`oxilabel`, crate `oxilabel-cli`) against a real server on a free port.
- `PYTHONPATH=worker python3 -m unittest discover -s worker/tests -t worker`: the pre-annotation worker's HTTP contract and shape helpers, with a fake adapter (no PyTorch needed).
- `scripts/ui-test.sh [name filter]`: builds the server and UI, then runs the browser flows in `scripts/ui-tests/tests.mjs` against a throwaway server and a headless Firefox driven over WebDriver BiDi (Node 22, no npm packages). Set `OXILABEL_UI_TEST_BROWSER` for another Firefox-family browser, e.g. `"flatpak run app.zen_browser.zen"`. Failures keep their screenshots under `target/ui-tests/`.
- In Leptos `view!` attributes, wrap any value containing `>` (closures with `=>`, `>=`, turbofish) in `{}`, or the macro ends the tag early.

## Commits and releases

- Commit with Cocogitto: `cog commit <type> "<message>" [scope]` (Conventional Commits; `cog.toml` holds the config). The `commit-msg` hook (`cog install-hook --all`) rejects non-conventional messages.
- Bump the version with `cog bump --auto` (or `--patch`/`--minor`/`--major`): it sets the workspace version via `cargo set-version`, updates `CHANGELOG.md`, commits and tags `vX.Y.Z`. Never edit the version or create release tags by hand.
