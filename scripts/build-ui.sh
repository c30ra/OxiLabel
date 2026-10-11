#!/usr/bin/env bash
# Builds the Leptos UI to crates/rtag-ui/dist (needs the wasm32 target and wasm-bindgen-cli).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/crates/rtag-ui/dist"
profile="${1:-release}"

if [ "$profile" = release ]; then flag=--release; else flag=; fi
cargo build --manifest-path "$root/Cargo.toml" -p rtag-ui --target wasm32-unknown-unknown $flag

rm -rf "$dist" && mkdir -p "$dist"
wasm-bindgen --target web --no-typescript --out-dir "$dist" \
  "$root/target/wasm32-unknown-unknown/$profile/rtag-ui.wasm"
cp "$root/crates/rtag-ui/index.html" "$root/crates/rtag-ui/style.css" "$dist/"
