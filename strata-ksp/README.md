# strata-ksp

Assemble a rocket, fork the launch, watch the two timelines come apart.

The screen has one subject: a history becoming two. The plot is altitude
against time, because two trajectories that differ by a tank of fuel sit
within a fraction of a percent of each other in world space and one is simply
drawn on top of the other. Under it runs a divergence strip — fork minus
parent, on its own scale — so the difference is a reading rather than
something you are asked to spot.

Two colours, and they mean one thing each: cyan is the timeline that kept
flying, amber is the one you forked off it. The plot reads in three acts.

| what you see | what it means |
| --- | --- |
| cyan alone | the parent flew this, and nothing had forked off it |
| amber dashes over cyan | a branch exists here and still agrees with its parent |
| amber solid, gap shaded | the branch was written to, and the gap is the change |

The stretch between the **BRANCH** rule and the **DIVERGED** mark is the point
of the whole thing: a fork costs nothing and changes nothing until you write
to it.

Catalog and golden are frozen (PR9). **Archive** deletes a launch; `design-*`
dies only at refcount 0. **Compare** is a counts DTO, not a promote dry-run.

```bash
cargo test
cargo run --release
# http://127.0.0.1:7430
```

While this process holds `./ksp-db`, `strata ./ksp-db branch list` fails with
`unavailable.engine.persistence`. Delete the directory to roll back.

## 90-second talk path

AUTO is on by default. Leave it on.

1. **Launch** — Sounding Stick from the pad, at 1×.
2. **Fork under power** — around T+2s, while the engine is still lit, hit
   **Fork from here**. `launch-0002` appears and the amber dashes run back
   along the history it inherited. Fork after the burn is over and the two
   will stay identical for the rest of the flight: a coasting trajectory does
   not depend on mass, so a tank added in orbit changes nothing. The
   divergence strip says so in as many words.
3. **Add tank** — **Add tank** on 0002, then **Run**. The strip opens up and
   keeps opening; the heavier stack out-climbs the one it was copied from.
4. **Strict refuse** — add a part in the hangar so both sides have moved, then
   **Promote design** with **Strict**. The verdict strip reports
   `conflict.engine.promotion · hangar unchanged`, and the hangar chip does not
   move. That is the demo.
5. **SourceWins** — flip the toggle and promote again. The hangar takes the
   forked stack and the graph is rebuilt; `design-0001` is not deleted.

The sentence the demo exists to make true: *I forked the launch mid-burn,
added a tank, and the other timeline is still in orbit.*
