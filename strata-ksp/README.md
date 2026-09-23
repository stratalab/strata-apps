# strata-ksp

Assemble a rocket, fork the launch, watch the other timeline stay in orbit.

The plot table is the product: a tungsten/paper canvas, IBM Plex, `launch-*` filmstrip sparklines, warp chips, and an AUTO lamp. Atmosphere stays out. Catalog and golden are frozen (PR9). **Archive** deletes a launch; `design-*` dies only at refcount 0. **Compare** is a counts DTO (not a promote dry-run).

```bash
cargo test
cargo run --release
# http://127.0.0.1:7430
```

While this process holds `./ksp-db`, `strata ./ksp-db branch list` fails with `unavailable.engine.persistence`. Delete the directory to roll back.

## 90-second talk path

AUTO is on by default. Leave it on.

1. **Launch** — Sounding Stick from the pad. Warp 10×.
2. **Orbit** — wait for status `orbit` on `launch-0001`. The filmstrip sparkline closes; the other trail is not a canvas polyline the UI owns.
3. **Edit hangar** — add a fin on the VAB while 0001 coasts. Save is already written by add.
4. **Strict refuse** — **Promote this design** with **Strict**. Expect `conflict.engine.promotion` and zero hangar mutation.
5. **SourceWins** — flip the toggle, promote again. VAB JSON becomes the orbiting stack; the graph is rebuilt; `design-0001` is not deleted.
6. **Scrub staging** — Pause. Drag **seq** back to the first staging event on 0001.
7. **Fork (shares design)** — **Fork-at**. Child `launch-0002` lists the same `design-0001`. Two sparklines, two `launches[]` entries.
8. **Add tank** — **+ tank** on 0002. Child wet mass rises; 0001 stays in orbit.
9. **Promote this design on 0002** — SourceWins (or Strict if the hangar still matches). The hangar takes the tanked stack.

The sentence the demo exists to make true: *I forked the launch at staging, added a tank, and the other timeline is still in orbit.*
