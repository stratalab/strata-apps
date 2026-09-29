# strata-paint

A paint program whose undo has no bottom.

> I drew for ten minutes, dragged the timeline back to the fourth stroke, and
> watched the other forty come off. Then I started painting again, and it
> forked — both versions, neither one lost.

Every stroke is a commit. The slider under the canvas is not an undo stack the
app is keeping — it is the database's own commit history, read back with
`get_versions`. Dragging it is an as-of read: `list_at("stroke/", timestamp)`
asks the engine which strokes existed at that point, and the canvas is redrawn
from the answer.

Procreate records a video to play a painting back. Here the playback *is* the
storage. Nothing was captured, because nothing was thrown away.

## What it shows

- **Time travel, visibly.** The claim on the website is "read any past version
  of your data". This is that sentence as something you can drag.
- **History that reaches.** `history_reaches_the_first_stroke_after_many`
  draws 301 strokes and reads the first one back. A retained window that only
  covered recent commits would pass a ten-stroke test and fail a real drawing.
- **Nothing deletes.** Clear is a wide erase stroke, so it takes a commit like
  anything else and you can scrub back through it. The eraser composites with
  `destination-out` rather than painting the background colour, so an erase at
  commit 4 is still an erase when you look at commit 40.

- **Branching from the past.** Scrub back and start drawing: the app calls
  `fork_at_timestamp` on the commit you are looking at and moves the brush to
  the new branch. There is no "branch here" button, because having scrubbed
  back and picked up the brush you have already said what you want. The
  painting you forked from keeps everything it had.

A fork inherits the history it was taken from, which is what makes the
scrubber work on a new branch: forking `default` at its third commit gives
`what-if` three commits of its own, not an empty timeline.

Merging is **not** here. Two timelines, either of which you can paint on.
KSP is the promotion demo.

## Shape

| file | what it holds |
| --- | --- |
| `src/stroke.rs` | what a stroke is, and what makes one invalid |
| `src/store.rs` | every database call, including the fork |
| `src/wasm.rs` | the browser bridge; JSON in, JSON out |
| `static/` | the UI: canvas, tools, scrubber |
| `tests/history.rs` | the time-travel claims, checked against the real engine |
| `tests/fork_at.rs` | that a branch really can start from a past commit |

The data model is two keys and one rule:

- `stroke/<8-digit seq>` holds one stroke, immutable once written
- `head` holds the count
- both go in through **one `put_batch`**, so a stroke costs exactly one commit
  and the timeline has one entry per stroke rather than two

Because a stroke never changes, a scrub can ask which keys existed at a
timestamp and then read each at latest — the two answers are the same one.
`a_scrub_reads_the_strokes_that_existed_not_the_ones_that_do` is the test that
should fail if a stroke ever becomes mutable.

## Running it

```sh
cargo test          # the store and the fork, against the real engine
./build-wasm.sh     # the browser build, into pkg/
./serve.py          # http://localhost:4400
```

`serve.py` exists only because `python3 -m http.server` has no mapping for
`.wasm`, so `WebAssembly.instantiateStreaming` rejects the response and
wasm-bindgen falls back to buffering the whole module — which works, but warns
on every load and buries real failures in the noise.

There is no native binary. Colonies and KSP each have a server because they
began as one; this is a canvas in a tab and has nothing to serve. The library
still compiles natively so the tests can drive the same store the browser does.

The database is the in-memory cache: `localfs` pulls fs2 and file locks that do
not exist on wasm32 (strata-core #3536). The painting lives as long as the tab,
and so does its history.
