#!/usr/bin/env bash
# Build the browser app: the engine compiled to wasm32 and driven from
# JavaScript. Produces a self-contained static directory in pkg/ - serve it
# from anywhere, there is no server side.
#
# Unlike Colonies and KSP there is no native build to stay in step with, so
# static/ is copied across rather than rewritten: one UI, one target.
#
# wasm-bindgen's generated glue and its CLI must agree on a schema version, so
# the crate is pinned with `=` in Cargo.toml and this checks the CLI matches
# before spending a release build on a mismatch.
set -euo pipefail
cd "$(dirname "$0")"

want=$(sed -n 's/^wasm-bindgen = { version = "=\([0-9.]*\)".*/\1/p' Cargo.toml)
have=$(wasm-bindgen --version | awk '{print $2}')
if [ "$want" != "$have" ]; then
  echo "build-wasm: wasm-bindgen CLI is $have, Cargo.toml pins $want." >&2
  echo "  cargo install -f wasm-bindgen-cli --version $want" >&2
  exit 1
fi

echo "==> cargo build (wasm32)"
cargo build --release --target wasm32-unknown-unknown --features wasm --lib

echo "==> wasm-bindgen"
rm -rf pkg
wasm-bindgen --target web --out-dir pkg --no-typescript \
  target/wasm32-unknown-unknown/release/strata_paint.wasm

echo "==> static"
cp static/index.html static/style.css static/app.js static/bridge.js pkg/

echo "==> done"
du -sh pkg
ls -1 pkg
