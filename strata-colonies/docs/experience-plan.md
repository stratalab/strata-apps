# Strata Colonies: branching timelines

Date: 2026-09-13

Status: slices 1–5 implemented locally; website publishing is a separate release action.

Slice 1 evidence and reproduction steps: [Browser feasibility](browser-feasibility.md).

Slice 2 scope and verification: [Visual foundation](visual-foundation.md).

Slice 3 scope and verification: [Interaction model](interaction-model.md).

Slice 4 scope and verification: [Branching experience](branching-experience.md).

Slice 5 scope and verification: [Website integration](website-integration.md).

## Current product direction

The follow-up [Play first](play-first.md) simplification supersedes the visible
control layout below. The main experience is Play, rewind, change a cell, and
try another future. Precision tools, comparison settings, lineage, and charts
are available only through Look closer.

## Purpose

Make a visitor understand StrataDB by changing one cell, watching the consequences,
revisiting the past, and trying another future. The release criterion is a complete
“fork a past moment, change a cell, compare the futures” journey.

Suggested title: **Strata Colonies**.

Suggested introduction:

> One cell. A different future.
> Change a colony, rewind its history, and branch a new experiment.

## Website integration and visual direction

Target route: `/demos/colonies/`, linked from the website's demos listing. The site
repository is `stratalab/stratadb.org`. Its current design
tokens, rather than the historical root DESIGN.md, are the visual reference.

- General Sans for headings, prose, and controls; Commit Mono for measurements.
- Black background, charcoal panels, white text, fine neutral borders.
- Ember orange (`#ff7a52`) for primary actions, active selection, and additions.
- Reuse the site's self-hosted fonts, spacing, radii, focus states, and tokens.
- Bright neutral live cells; distinguish additions and missing cells with both
  color and shape. Keep colony identity separate from cell comparison meaning.
- Restrained transitions, sharp canvas rendering, and reduced-motion parity.
- Remove the engine-findings section, paths, hashes, commit counters, and
  persistence timings from the main visitor experience.

## Layout

| Region                                 | Contents                                                                 |
| -------------------------------------- | ------------------------------------------------------------------------ |
| Main stage, about 70% of desktop width | Large selected colony, name, generation, comparison overlay              |
| Right sidebar, about 30%               | Play/pause, step, speed, edit tools, Fork from here, comparison selector |
| Below stage                            | Scrubbable timeline with mutation and fork markers                       |
| Below timeline                         | Compact branch previews and divergence chart                             |

Controls remain available during interaction. Mobile gets a compact sticky
transport bar and an expandable controls drawer. Selection updates the canvas,
timeline, and sidebar together.

## Starting experience

- Start with one control and five single-cell variants: six visible colonies.
- Use deterministic starting patterns and validate that chosen mutations produce
  interesting visible outcomes.
- Open paused at generation zero with **Run experiment** prominent.
- Invite participation: “Select a colony. Click a cell to change its future.”
- Preserve the control as a reference; editing it starts a new experiment.
- Initially cap the experiment at 12 visible branches; validate memory/history
  limits in the feasibility slice before fixing the public limits.
- Additional soup, glider, and oscillator presets are a later enhancement.

## Editing

Entering edit mode pauses playback. Hover identifies the cell. Clicking toggles;
dragging paints or erases. Pending edits stay highlighted. Undo, Clear edits,
and Run with changes make changes deliberate and reversible. A group of edits
becomes one mutation checkpoint, with before/after inspection available.

Example context: “Editing Experiment 2 at generation 48 · 3 cells changed.”

## History and forks

1. Run Experiment 2 to generation 120.
2. Scrub back to generation 48; the UI explicitly says Viewing history.
3. Select Fork from here.
4. Edit the new branch, then run it.
5. Compare the result with the parent's recorded future at the same generation.

Historical inspection performs reads and does not overwrite current state.
Historical edits create a child branch. The parent's future remains intact.
Playback from a historical position replays recorded history until the head;
Return to latest exits inspection. Branch labels record parent and fork generation.

Each branch owns its head generation and checkpoints. A checkpoint identifies
generation **and revision**, since multiple edits can occur without a Life step.
Simulation generation, Strata commit version, logical timestamp, and wall-clock
time are distinct fields and must never be substituted for one another.

## Comparison and explanation

- Compare with Control or Parent branch at matching simulation generations.
- Offer overlay and optional side-by-side views.
- Show a readable difference count and a chart with labeled generation axes.
- Mark fork points and preserve a clickable lineage tree.
- Explain unavailable comparisons rather than comparing different generations.
- A collapsed Powered by StrataDB panel can explain boards (KV), metadata (JSON),
  mutations (events), and lineage (graph). Only show actual engine behavior.

## Browser architecture

Prefer the real Strata WebAssembly engine in a dedicated Web Worker. The existing
website is static Astro deployed to GitHub Pages and already ships the browser
engine. Each visitor gets a separate cache-mode database, with no shared server
simulation. The native Rust application remains the local reference during this work.

The deployed browser engine was inspected on 2026-09-13 and reported 1.2.2.
A retained-version fork was tested against it and returned the earlier value.
Slice 1 verified performance and retained history through 1,000 generations at
six and twelve branches; see its measurement report for device limits.

The browser engine is volatile: reloading or closing the session discards it.
Do not imply browser durability. Export/import or shareable experiment recipes
can follow the core experience. Use released, version-checked WASM artifacts.

Keep simulation logic and the engine adapter separate from the eventual website
shell. Worker messages should cover initialization, step/run, historical reads,
forking, editing, inspection, and disposal. Errors must produce actionable UI
states. UI rendering must not run inside the worker or depend on synchronous
database calls on the main thread.

## Delivery slices

### 1. Browser feasibility

Build a reproducible local harness using real WASM in a module worker. Validate:

- Six 64×48 colonies with deterministic Life evolution.
- KV board snapshots, JSON status, and event checkpoints with real commit facts.
- Exact historical reads after later steps and edits, including same-generation revisions.
- Historical forks, independent children, preserved parent future, and aligned comparisons.
- Separate visitor sessions and clean worker disposal.
- Retention at a useful history depth and failure without silently substituting head state.
- Worker throughput, frame responsiveness, and memory observations at 6 and 12 branches.
- Parity of the browser Life implementation with the native Rust implementation.

Record reproducible commands, artifact version/hash, test outcomes, benchmark
parameters, and limitations in `docs/browser-feasibility.md`. A developer harness
is sufficient for this slice; the public design belongs to slice 2.

Provisional targets: six colonies at 8 generations/second (125 ms budget per
world step), history inspection/fork actions under 100 ms on the reference machine,
and at least 1,000 retained generations. Measure 12 branches as a headroom check.
These are local engineering gates, not published performance claims.

### 2. Visual foundation

Adopt shared tokens/fonts and implement the main canvas, right controls, six
previews, responsive shell, and initial invitation. Review in real browser sizes.

### 3. Interaction model

Add editing, undo, grouped checkpoints, scrubbing, and unambiguous historical/live
playback states. Preserve recorded outcomes throughout.

### 4. Branching experience

Expose dynamic colonies, historical forks, lineage, and generation-aligned
comparisons. Test nested forks and multiple edits within a generation.

### 5. Polish and website integration

Finish onboarding, keyboard/touch behavior, accessibility, loading/error states,
motion, and browser validation. Integrate the website route and demos listing.
Run the website's applicable quality gates before publishing.

## Completion criteria

A first-time visitor can run the starter experiment, inspect a past generation,
fork it, introduce a visible mutation, and compare both futures without losing
their place. All represented branching/history behavior comes from real Strata
operations. The page matches the website's design and remains responsive during
database work. Mobile interaction, errors, and session lifetime are explicit.
