# Stockfish (vendored, unmodified)

This directory holds **Stockfish**, which is **not** part of strata-chess and is
**not** under the same licence as the rest of this repository. It is
[GNU General Public License v3](Copying.txt), and it is included here so the
demo can be served as a single static bundle.

| | |
| --- | --- |
| Program | Stockfish 19 Lite (WASM, single-threaded) |
| Files | `stockfish-19-lite-single.js`, `stockfish-19-lite-single.wasm` |
| Source of these binaries | npm `stockfish@19.0.0` |
| Modified? | **No.** Copied byte for byte from that package. |
| sha256 (js) | `d3344124ab067fb0…` |
| sha256 (wasm) | `57ac2d72312aba34…` |
| Licence | GPL-3.0, full text in [`Copying.txt`](Copying.txt) |

## Corresponding Source

The Corresponding Source for these binaries is the Stockfish source together
with the Emscripten build scripts that produced them:

- Build wrapper: <https://github.com/nmrugg/stockfish.js> (the project that
  publishes npm `stockfish`), at the tag corresponding to 19.0.0
- Stockfish itself: <https://github.com/official-stockfish/Stockfish>

GPLv3 section 6(d) allows conveying object code by offering access from a
designated place, provided there are clear directions beside the object code
saying where to find the source. That is what this file is, and it ships in the
same directory as the binaries wherever they are served.

## Why it is a Worker

Stockfish runs as a Web Worker: a separate program, started at runtime, talking
UCI over `postMessage`. It is not linked into the app's own WebAssembly module,
which is built from this repository's Rust and carries this repository's
licence. The two are aggregated on a page, not combined into one program.

## Keeping this honest

If these binaries are ever replaced, rebuilt or patched, this file has to change
with them: the version, the hashes, and the "Modified?" row. `cargo test` checks
the hashes so a silent swap fails the suite rather than shipping a licence
notice that describes a different binary.
