# Map-centered navigation

The map occupies the full viewport. A floating panel opens one activity at a time:

- **Explore:** search, category filters, five suggested destinations, and optional
  discovery by road distance.
- **Directions:** origin/destination selection and route results.
- **Missions:** the existing landmark passport, progress, and simulated travel.
- **Closures:** street selection, scenarios, detours, reopening and history.

The panel can collapse without moving or resizing the map. On mobile it becomes
a bottom panel capped at 48% of viewport height, with its own scroll area. The
document never scrolls away from the map. Map layers, overview, theme and system
information live in a compact floating toolbar. The header/status/time and bottom
console bars are removed. Routine successful route requests do not show a toast;
loading, actionable errors and temporary interaction instructions remain available.

Selecting a place from search or the map opens a nonmodal card within the panel,
highlights the location and zooms into the unobstructed map area. The camera
accounts for the desktop panel or mobile bottom panel. Map panning, zooming and
other place selections remain available. Back restores the previous place and
camera, or the search results; close/Escape returns to the activity. Card history
is bounded to 20 entries. Directions/From here move into the Directions activity.
Source attribution, approximate road access and graph relationships are under
**About this place**. Subway services and exploration stay available on station
cards. Road routing, dataset versions and database contents are unchanged.

Activity tabs support arrow keys, Home and End. Place headings receive focus on
selection; dismissal returns focus to the opener when possible. Place cards do
not trap focus. Reduced-motion preferences apply to camera movement and journeys.
Switching activities ends street-picking mode and preserves the draft selection.

Validation: `tools/check_map_ux.cjs` checks the full-screen canvas, exclusive modes,
keyboard navigation, zoom and selected-pin visibility, map interaction with a card
open, card history, directions, collapsing, mobile bounds and dark mode. The
existing UI, subway and interactive mission checks now navigate through the tabs
and continue to exercise the actual graph and scenario APIs on a disposable
cache-mode server. Static assets are embedded in the Rust binary, so rebuild and
restart to serve a new interface.
