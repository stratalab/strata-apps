# strata-chess

Chess positions to solve, and a database that keeps every line you tried.

> I played the natural move, and it told me I had thrown the draw away. I
> clicked back to the start, found the study move, and the losing line was
> still sitting there in the tree.

A chess GUI keeps its variation tree in memory and writes it out as PGN with
brackets in it. Here a variation **is** a branch: playing a different move from
a position you have already been to calls `fork_at_timestamp` on that commit,
and the list of lines in the sidebar is the branch list read back from the
engine. One commit per half-move.

That is the whole point, and it is the thing chess study actually needs. A
wrong try costs nothing, because nothing is overwritten to make room for the
right one.

## The five

| level | position | what it is |
| --- | --- | --- |
| beginner | The back rank | `Ra8#`. A wall of his own pawns. |
| easy | Smothered | `Nf7#`. Every flight square taken by his own men. |
| medium | Two at once | A knight fork that wins the rook on a8. |
| hard | One pawn, one idea | King and pawn against king. Every move is the only move. |
| grandmaster | Réti's square | Two pawns, a king nowhere near either, and a draw. |

Every one was checked with the same engine that defends it, not recalled:
the mates are mate in one, the fork really wins the rook, the king-and-pawn
ending really is won and Réti really is a draw. Four other candidates were
thrown out on the evidence - two "wins" the engine scored dead level, one
illegal FEN, and a Greek gift where it preferred a quiet move to the sacrifice.

## What is whose

| | |
| --- | --- |
| Rules, move generation | [`cozy-chess`](https://github.com/analog-hors/cozy-chess), MIT |
| Opponent and analysis | **Stockfish 19 Lite**, GPL-3.0, see [`vendor/stockfish/`](vendor/stockfish/) |
| Everything else | this repository's licence |

**Stockfish is not part of this program.** It is vendored unmodified, with its
licence and directions to its Corresponding Source beside it, and it runs as a
Web Worker: a separate program, started at runtime, spoken to over UCI. Nothing
from it is linked into the WebAssembly built here. `build-wasm.sh` copies the
licence and the notice into the served bundle, because GPLv3 asks for them to
be next to the object code and nobody serving `pkg/` would otherwise see them.
`tests/vendor.rs` checks the binaries still hash to what the notice claims, so
a rebuild cannot quietly leave the notice describing a different file.

The single-threaded "lite" build is not a preference. GitHub Pages cannot send
the COOP and COEP headers that `SharedArrayBuffer` requires, so a threaded
build cannot start there at all. This one needs neither, and it still searches
to depth 14 in well under a second.

## The two things this crate owns

Move generation is somebody else's, deliberately: it is where a bug is both
fatal and invisible, producing a position that looks fine and is not legal.
What is written here is the two translations between that library and a board
a person is looking at.

1. **Castling.** `cozy-chess` speaks Chess960, where a castle is the king
   taking its own rook: `e1h1`, not `e1g1`. Somebody dragging a king two
   squares means the second. The two spellings meet in one function and
   nowhere else.
2. **SAN.** Including disambiguation, which is the part that gets skipped and
   then prints `Ne4` for a position where two knights can reach e4.

`tests/rules.rs` runs perft against the published numbers from three standard
positions - the start (20 / 400 / 8902 / 197281), Kiwipete (48 / 2039 / 97862)
and the en-passant-pin position (14 / 191 / 2812 / 43238). A generator that is
wrong anywhere misses them.

## Running it

```sh
cargo test          # perft, SAN, the scenarios, the vendored licence
./build-wasm.sh     # the browser build, into pkg/
./serve.py          # http://localhost:4410
```

There is no native binary. The library compiles natively so the tests can drive
the same rules and the same store the browser does.
