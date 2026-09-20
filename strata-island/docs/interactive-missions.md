# Custom closures and exploration missions

Open **Closures** for the street editor or **Missions** for the passport. Each activity now has its own floating panel; see [map navigation](map-ux.md).

The V2 console defaults to a custom closure editor. Select **Choose streets on the map**, then **Select streets on map**; click a section to toggle it. Drag/pinch still pans and zooms, and Escape ends selection. The searchable street-section list provides a keyboard-accessible alternative. Searches distinguish numeric street names such as 34th and 134th. Labels include cross streets where available and approximate section length.

Selected sections preview in orange. Undo, remove individual sections, or clear the selection; give the closure a name and create a scenario. Each physical section closes all existing travel directions, including the reverse direction only when it exists. The application enforces the existing 200-directed-edge limit and refuses additions beyond it. The official city stays unchanged. Existing scenario history, reopen, compare and archive actions apply to the custom closure. Current-route (one travel direction) and 42nd Street presets remain available.

## Landmark Passport

Three missions each contain three fixed, source-identified destinations:

- Midtown icons: Rockefeller Center, Empire State Building, MoMA.
- Museum circuit: the Met, Guggenheim and American Museum of Natural History.
- Downtown green spaces: Washington Square, Union Square and Madison Square parks.

Start from the current route origin. Each **Route to next stop** action calls `/api/discover` on the current scenario or official city. Strata's directed SSSP distances determine the nearest reachable unvisited mission destination. The existing route service reconstructs its path. This is a nearest-next-stop mission, not a claim of globally optimal tour planning.

**Travel route** animates a simulated journey over that valid path and records a visit when it finishes. Reduced-motion preferences skip the animation. A missing scenario route cannot count as a visit; if every remaining stop is unreachable, the mission explains that the corridor must be reopened or another scenario selected. Endpoint and mutation controls are locked during a journey. A completed stop cannot be counted twice within a mission.

Passport visits, active mission progress and total simulated route distance are saved in `localStorage` under `island-passport-v1`. Badges are derived from collected landmarks, so they remain visible after starting another mission and after reload. This is browser-local progress, not Strata-persisted gameplay, GPS tracking, or a competitive leaderboard. It also works for the current session when storage is unavailable.

The feature reuses the existing scenario APIs and graph primitives; it does not change the graph schema, catalog revision or database format. Map previews and journey animation run locally without per-frame graph requests. Existing Strata limitations remain tracked in the friction ledger; no new engine issue was observed during these checks.

## Verification

`tools/check_interactive.cjs` runs against a disposable cache server and refuses durable servers. It checks map and search selection, ordinal street-name matching, undo/clear, exact directed-edge removal and parent isolation, reopening, animated/reduced-motion journeys, all three missions, an actual closure that makes the mission origin unreachable, double-visit prevention, passport/badge persistence, and mobile layout. `tools/check_ui.cjs` retains the original routing/discovery/42nd-preset flows.
