#!/usr/bin/env bash
# Builds the server and the UI, then runs the browser tests in scripts/ui-tests.
# Pass a name filter to run only matching tests. Set OXILABEL_UI_TEST_BROWSER to use another
# Firefox-family browser, e.g. OXILABEL_UI_TEST_BROWSER="flatpak run app.zen_browser.zen".
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --release --manifest-path "$root/Cargo.toml" -p oxilabel-server
"$root/scripts/build-ui.sh"
exec node "$root/scripts/ui-tests/run.mjs" "$@"
