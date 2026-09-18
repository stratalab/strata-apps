# Continuous playback and recent history

Choosing an Other futures preview changes the displayed colony without pausing
the simulation. All colonies continue advancing on the same worker timer. If the
simulation is already paused, switching keeps it paused. Opening a specific past
moment or editing still pauses so the selected board can be inspected safely.

There is no generation stop. Each colony keeps its most recent 1,200 checkpoints,
including same-generation edits. The oldest checkpoint rolls out as the next one
arrives. Rewind shows the actual oldest retained generation instead of Beginning
once that happens. A child survives expiry of its parent's fork point; lineage
keeps the origin metadata and disables navigation to the expired moment.

## Database storage

The pinned Strata 1.2.2 WASM command surface does not expose an operation for
trimming arbitrary old MVCC versions. Deleting a current key or hiding checkpoint
indices would not reclaim the earlier versions and append-only event rows.

The app therefore stores history in bounded groups of temporary Strata databases.
After 1,536 checkpoint writes, new writes go to a fresh segment. A continuation
writes the colony's next board and metadata there; previous checkpoints remain
in their original databases. Checkpoint IDs include the segment ID because
database commit versions are scoped to each database. Reads route to that segment
and use its native timestamp-based KV and JSON reads. All database work stays in
the worker.

A user fork still calls `branch_fork_at_version` inside the source checkpoint's
database and verifies the inherited board before any child write. It is not
replaced by a JavaScript copy masquerading as a native fork. Later segments
materialize continued states with ordinary writes; cross-segment lineage is app
metadata, rather than one native branch spanning several databases.

Once no retained checkpoint refers to a segment, the app frees its entire
`StrataSession`, releasing the stored versions and event rows. This is batched
storage reclamation: some expired rows can remain until the last retained moment
in their segment expires. An idle colony's retained moment also keeps its segment
alive. No old segment is held solely because a child has origin metadata there.
WASM linear memory does not shrink, but freed allocations are reused.

History limits apply to each colony independently; generation numbers never
reset. The app still supports at most 12 colonies, and refreshing starts a fresh
temporary experiment. No Strata core or released WASM changes are required.

## Verification

`npm run test:retention` uses the actual pinned WASM to run 6,301 generations,
checking the 1,200-checkpoint window, retained boards against the Life stepper,
reads and native forks across segment boundaries, expired requests, mutation-only
windows, idle colonies, and child independence after source expiry. It verifies
that old sessions are freed and every session is freed on disposal.

In the six-colony run, WASM linear memory reached 33,751,040 bytes by generation
2,000 and stayed there at generations 3,000, 4,000, and 5,000. The run held at most
six database segments at those sampling points. These are measurements on the
development machine, not a universal browser memory guarantee. Results are in
`artifacts/retention-verification.json`.

The Chromium, Firefox, and WebKit release tests switch live previews and verify
that generations keep advancing. A smaller test window exercises actual timer
ticks through expiry, bounded rewind, a fork from the oldest retained moment,
and disabled origin links after expiry. The existing editing, branching,
accessibility, and Rust/browser Life parity checks also remain in place.
