#!/bin/sh
# Builds the engine for the browser: crates/loci-web → wasm32-unknown-unknown (release) →
# wasm-bindgen (web target) into apps/loci/wasm/, which the browser build imports as `@loci-web`.
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the version in Cargo.lock.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
cargo build -p loci-web --target wasm32-unknown-unknown --release
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
wasm-bindgen --target web --out-dir "$here/wasm" --out-name loci_web "$target/wasm32-unknown-unknown/release/loci_web.wasm"
echo "wasm engine built in $here/wasm"
