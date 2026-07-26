# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

The Phase 1 MVP (see Roadmap below) is implemented twice over, sharing one
engine: a Cargo workspace with `crates/sdts-engine` (portable
scenario/scoring/protocol/recording logic, no Axum/Tokio/WebSocket/browser
dependency, compiles for native Rust and `wasm32-unknown-unknown`),
`crates/sdts-server` (the native Axum app — browser renderer + mouse
impact adapter over SDTP/WebSocket, with recording and replay), and
`crates/sdts-wasm` (a `wasm-bindgen` wrapper around the engine, driven
directly by the browser demo in `docs/` with no server at all). See
[docs/index.md](docs/index.md)'s "Browser WASM demo" section for the full
breakdown, and `README.md` for the live demo URL.

- Build (both native crates): `cargo build --workspace`
- Test: `cargo test --workspace`
- Run the native server (live recording, default): `cargo run -p sdts-server` — serves on http://127.0.0.1:3000
- Run the native server (replay a specific file from the CLI): `cargo run -p sdts-server -- replay recordings/<file>.jsonl [speed]`
- Build/rebuild the browser WASM demo: `make build` (or `make wasm` +
  `make scenarios` separately); serve it locally with `make serve`. Output
  (`docs/pkg/`, `docs/scenarios/*.json`) is gitignored — `scenarios/` and
  `crates/sdts-wasm` are the source of truth, and
  `.github/workflows/pages.yml` rebuilds and deploys `docs/` on every
  push to `main`, so there's nothing to commit after a local build.
- Verify the WASM engine also compiles standalone: `cargo build -p sdts-engine --target wasm32-unknown-unknown`

The native server's active session (live recording vs. replaying a past
file) is not fixed for the process lifetime — `AppState`
(`crates/sdts-server/src/main.rs`) holds it behind a lock and swaps it at
runtime via `POST /api/session/record` and `POST /api/session/replay`,
which `static/index.html`'s Record/Replay tabs call so a user can record
and replay from the same page without restarting the server. `GET
/api/recordings` lists `recordings/*.jsonl` for the Replay tab's picker.
Each new/replaced session gets its own broadcast channel;
`crates/sdts-server/src/ws.rs` snapshots the current session once per
WebSocket connection, so the page closes and reopens its socket after
every mode switch to pick up the new one. `static/index.html` (native,
WebSocket-based) and `docs/index.html` (browser-only WASM) are two
separate demo pages — don't conflate them when making rendering/UI
changes; check which one a request is actually about.

## Vision

OpenSDTS (Open Software Defined Target System) separates **what a target is**
from **how it is displayed** and **how impacts are measured**. The goal is to
support multiple renderers (laser, monitor, projector) and multiple impact
detectors (mouse, ShotMarker, FreeETarget) through one common architecture,
so hardware can be swapped without changing scenario/scoring logic.

## Architecture (see `docs/architecture.md`, `docs/components.md`)

```
Scenario
   │
   ▼
 SDTS Core
   ├── Renderer
   ├── Impact Adapter
   ├── Scoring
   └── Recorder
```

- **Core** — owns and maintains simulation/game state.
- **Renderer** — displays targets (planned: Browser, ILDA, LightElf, Ether Dream).
- **Impact Adapter** — converts device-specific input into protocol messages
  (planned: Mouse, ShotMarker, FreeETarget).
- **Scoring** — compares core state against impact adapter reports.
- **Recorder** — stores every protocol message for replay.

All components communicate exclusively via **SDTP** (see below); components
should not share state or call into each other directly.

## SDTP (protocol, see `docs/protocol.md`)

JSON messages over WebSocket. Envelope shape:

```json
{
  "type": "impact",
  "time": 1.23,
  "source": "mouse",
  "data": {}
}
```

Rules:
- Units are millimeters; time is seconds elapsed since session start.
- Events are immutable once emitted.
- Unknown fields must be ignored by consumers (forward compatibility).

## MVP (see `docs/mvp.md`)

The MVP exists to prove the architecture before investing in additional
hardware. Scope is intentionally narrow:
- Rust SDTS Core
- Browser renderer
- Mouse impact adapter
- Flow: spawn a moving circle → render in browser → click to simulate
  impacts → compute hit/miss → record every event.
- Success criteria: circle motion is deterministic, replay reproduces
  identical motion, hits score consistently.

## Roadmap (see `docs/roadmap.md`)

1. Browser renderer, mouse impacts, circle target, replay.
2. Projector integration, calibration, laser renderer.
3. ShotMarker integration, multiple targets, competition scenarios.
4. Additional hardware, plugin ecosystem.

Keep new work aligned to the current phase rather than implementing
later-phase hardware integrations early.
