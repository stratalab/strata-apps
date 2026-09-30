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
  target/wasm32-unknown-unknown/release/strata_chess.wasm

echo "==> static"
cp static/index.html static/style.css static/app.js static/bridge.js static/engine.js static/generate.js pkg/

# Stockfish travels with its licence. GPLv3 asks for the licence text and clear
# directions to the Corresponding Source beside the object code, so the notice
# and Copying.txt are copied into the served directory rather than left behind
# in the repository where nobody serving the bundle would see them.
echo "==> stockfish (GPL-3.0, unmodified, see vendor/stockfish/README.md)"
mkdir -p pkg/stockfish
cp vendor/stockfish/stockfish-19-lite-single.js \
   vendor/stockfish/stockfish-19-lite-single.wasm \
   vendor/stockfish/Copying.txt \
   vendor/stockfish/README.md pkg/stockfish/

echo "==> done"
du -sh pkg
ls -1 pkg
