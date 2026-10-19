#!/usr/bin/env bash
# Builds the Leptos UI to crates/oxilabel-ui/dist (needs the wasm32 target and wasm-bindgen-cli).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/crates/oxilabel-ui/dist"
profile="${1:-release}"

if [ "$profile" = release ]; then flag=--release; else flag=; fi
cargo build --manifest-path "$root/Cargo.toml" -p oxilabel-ui --target wasm32-unknown-unknown $flag

rm -rf "$dist" && mkdir -p "$dist"
wasm-bindgen --target web --no-typescript --out-dir "$dist" \
  "$root/target/wasm32-unknown-unknown/$profile/oxilabel-ui.wasm"
cp "$root/crates/oxilabel-ui/index.html" "$root/crates/oxilabel-ui/style.css" "$dist/"
