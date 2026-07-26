# OpenSDTS

**Open Software Defined Target System**

OpenSDTS is a platform for creating, rendering, and scoring dynamic target scenarios.

Define a scenario once, run it anywhere.

Today that means a browser with mouse input. Tomorrow it could be a projector, laser system, electronic target, or any other renderer and impact detector.

---

## 🎯 Live Demo

**[https://zhagan.github.io/SDTS/](https://zhagan.github.io/SDTS/)**

The real OpenSDTS Rust engine — scenario evaluation, scoring, recording, and
replay — compiled to WebAssembly and running entirely in your browser. No
server, no backend, no install.

<video src="https://zackhagan.com/SDTS/demo_video/sdts-demo.mp4" controls width="640"></video>

---

## What is it?

A scenario is described declaratively.

```text
Load Scenario
      │
      ▼
  SDTS Core
      │
      ├── Renderer
      ├── Impact Adapter
      ├── Scoring
      └── Recorder
```

The Core owns the authoritative state of every target.

Renderers display targets.

Impact adapters report impacts.

The scoring engine evaluates hits.

The recorder captures every event for deterministic replay.

---

## Two builds, one engine

All scenario parsing/validation, deterministic scenario evaluation, impact
scoring, SDTP event creation, and recording/replay logic lives once, in
[`crates/sdts-engine`](crates/sdts-engine) — a plain Rust crate with no
dependency on Axum, Tokio, WebSockets, the filesystem, or browser APIs. Two
thin builds consume it:

-   **`crates/sdts-server`** — the native Axum/WebSocket server (unchanged
    in spirit from the original Phase 1 MVP): browser renderer + mouse
    impact adapter talking SDTP over a WebSocket, with recording to
    `recordings/*.jsonl` and replay of past sessions.
-   **`crates/sdts-wasm`** — a thin `wasm-bindgen` wrapper around the same
    engine, compiled to WebAssembly and driven directly by
    [`docs/app.js`](docs/app.js) with no server in between (this is what
    powers the Live Demo above).

Neither build re-implements engine behavior in JavaScript or duplicates it
across crates — see [docs/index.md](docs/index.md) for the full breakdown.

---

## Current Features

* Browser renderer
* Moving targets
* Hit / miss scoring
* Session recording, export, import, and replay (native and browser/WASM)
* Declarative JSON scenarios
* Timed sequences
* Deterministic randomization
* Multiple target path types
* Browser scenario selection
* JSON protocol over WebSocket (native server) or directly in-process (browser/WASM)

---

## Example Scenario

```json
{
  "name": "Random Pop-up",
  "seed": 42,
  "targets": [
    {
      "display": {
        "shape": "circle",
        "radius_mm": 75
      },
      "repeat": true,
      "sequence": [
        {
          "action": "show",
          "duration_secs": 3,
          "position": {
            "type": "random"
          }
        },
        {
          "action": "hide",
          "duration_secs": {
            "random": {
              "min": 1,
              "max": 3
            }
          }
        }
      ]
    }
  ]
}
```

---

## Design Goals

* Hardware independent
* Deterministic execution
* Replayable sessions
* Simple protocol
* Declarative scenarios
* Modular architecture
* Open source

---

## Long-Term Vision

OpenSDTS separates **what a target does** from **how it is displayed** and **how impacts are measured**.

This allows the same scenario to run against different combinations of:

* Browser renderer
* Video projector
* Laser projector
* Electronic target systems
* Future renderers

and

* Mouse input
* Electronic impact detectors
* Future impact adapters

without changing the scenario definition.

---

## Running it

**Browser (no server, GitHub Pages-compatible):**

```bash
make build      # wasm-pack build + copy scenarios into docs/
make serve      # static file server over docs/, e.g. http://localhost:8080
```

**Native server:**

```bash
cargo run -p sdts-server                              # live recording, http://127.0.0.1:3000
cargo run -p sdts-server -- replay recordings/<f>.jsonl [speed]
```

See [docs/index.md](docs/index.md) for full build/run/deploy instructions,
including how to publish the browser demo via GitHub Pages.

---

## Repository Layout

```text
crates/
  sdts-engine/    portable scenario/scoring/protocol/recording engine
  sdts-server/    native Axum/WebSocket server
  sdts-wasm/      wasm-bindgen wrapper around sdts-engine
docs/
  index.html      browser demo entry point (GitHub Pages root)
  app.js, styles.css, pkg/, scenarios/
  *.md            architecture/protocol/scenario documentation
scenarios/        scenario definitions (source of truth; copied into docs/scenarios/)
recordings/       native server's recorded sessions (*.jsonl)
```

---

## Documentation

* [Architecture](docs/architecture.md)
* [Components](docs/components.md)
* [Declarative scenarios](docs/scenarios.md)
* [SDTP Protocol](docs/protocol.md)
* [MVP](docs/mvp.md)
* [Roadmap](docs/roadmap.md)
* [docs/index.md](docs/index.md) — full documentation index, including the browser WASM demo

---

## Status

OpenSDTS is currently focused on building a robust reference implementation using a browser renderer.

Future work will add additional renderers, impact adapters, and more sophisticated target scenarios while keeping the Core and scenario format unchanged.
