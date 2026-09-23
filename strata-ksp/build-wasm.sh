#!/usr/bin/env bash
# Build the browser version: the same simulation and the same engine as the
# binary, compiled to wasm32 and driven from JavaScript instead of HTTP.
#
# Produces a self-contained static directory in pkg/ - index.html, app.js,
# style.css, the bridge, and the wasm. Serve it from anywhere; there is no
# server side.
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

echo "==> cargo build (wasm32, no localfs)"
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm --lib

echo "==> wasm-bindgen"
rm -rf pkg
wasm-bindgen --target web --out-dir pkg --no-typescript \
  target/wasm32-unknown-unknown/release/strata_ksp.wasm

echo "==> static"
cp static/style.css static/app.js static/bridge.js pkg/

# One UI. The browser shell is generated from the one the binary serves, so
# they cannot drift: the only difference is that app.js is loaded by the
# bridge once the engine is up, rather than directly.
python3 - <<'PY'
import pathlib, re
src = pathlib.Path("static/index.html").read_text()
out = src.replace(
    '    <script src="/app.js"></script>\n',
    '    <p id="boot" class="boot">Loading the engine…</p>\n'
    '    <script type="module" src="./bridge.js"></script>\n',
)
assert 'bridge.js' in out, "index.html no longer ends with the app.js tag"
out = out.replace('href="/style.css"', 'href="./style.css"')
pathlib.Path("pkg/index.html").write_text(out)
PY

echo "==> done"
du -sh pkg
ls -1 pkg
