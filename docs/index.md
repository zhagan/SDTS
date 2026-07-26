# OpenSDTS Docs

- [Demo video](demo_video/sdts-demo.mp4) — a recorded walkthrough of the browser demo in action.
- [Architecture](architecture.md) — components and how they fit together (Core, Renderer, Impact Adapter, Scoring, Recorder).
- [Components](components.md) — responsibilities of each component.
- [Protocol (SDTP)](protocol.md) — the WebSocket/JSON message envelope shared by every component.
- [Declarative scenarios](scenarios.md) — target displays, paths, timed sequences, and seeded randomness.
- [MVP](mvp.md) — scope and success criteria for the first working build.
- [Roadmap](roadmap.md) — phased plan from the browser MVP through additional hardware.

## Browser WASM demo

**Live:** [https://zhagan.github.io/SDTS/](https://zhagan.github.io/SDTS/) — entry point [`index.html`](index.html).

### One engine, two builds

[`crates/sdts-engine`](../crates/sdts-engine) is a plain Rust library with
no dependency on Axum, Tokio, WebSockets, the filesystem, or browser APIs.
It owns everything the Core is responsible for per [Architecture](architecture.md):
scenario parsing/validation, seeded deterministic scenario evaluation
(`Scenario::states_at`), impact scoring (`scoring::evaluate`), SDTP event
creation (`protocol::*`), and session recording/replay
(`engine::Engine`). It compiles for native Rust and for
`wasm32-unknown-unknown` unchanged.

Two thin builds consume that one engine — neither reimplements any of its
logic:

- [`crates/sdts-server`](../crates/sdts-server) — the native Axum app.
  Its live-session ticker calls `Engine::update`, and its WebSocket impact
  handler calls `Engine::impact`; it broadcasts the resulting SDTP
  envelopes and writes them to `recordings/*.jsonl` exactly as the
  original Phase 1 MVP did. Replay is unchanged: the server just replays
  raw recorded envelopes over the socket (no scenario re-evaluation, no
  scoring), so it needs no `Engine` at all.
- [`crates/sdts-wasm`](../crates/sdts-wasm) — a `wasm-bindgen` wrapper
  (`SdtsEngine`) around the same `Engine`, compiled to a browser ES module.
  [`app.js`](app.js) calls it directly — there is no server, no
  WebSocket, and no JavaScript reimplementation of scenario/scoring logic.
  `app.js` only does canvas rendering, pointer input, and (de)serializing
  the small JSON payloads the engine returns.

The native server is push-based (it streams `target_update`/`result`
messages down a WebSocket as they happen); the browser build is poll-based
(`app.js` calls `update()`/`impact()` then reads `snapshot_json()` once per
`requestAnimationFrame`, passing the absolute elapsed session time rather
than a fixed frame delta). Both drive the identical underlying
`Scenario::states_at`/`scoring::evaluate` logic, so a scenario behaves
identically either way.

### Building the WASM package

```bash
make wasm       # wasm-pack build crates/sdts-wasm --target web --out-dir ../../docs/pkg
make scenarios  # copy scenarios/*.json into docs/scenarios/ + generate manifest.json
make build      # both of the above
```

Requires `wasm-pack` and the `wasm32-unknown-unknown` target
(`rustup target add wasm32-unknown-unknown`). `docs/pkg/` and
`docs/scenarios/*.json` are generated output (gitignored, not checked
in) — `scenarios/*.json` and `crates/sdts-wasm` are the source of truth.
Run `make build` locally when you want to `make serve` and preview the
demo; deployment builds fresh in CI (see below), so there's nothing to
commit here.

### Running it locally

```bash
make serve      # python3 -m http.server 8080 --directory docs
```

Then open `http://localhost:8080/`. Every asset reference in `docs/` is a
relative path (`./pkg/sdts_wasm.js`, `scenarios/manifest.json`, ...), so
the same files work unmodified from a local static server or from
`https://zhagan.github.io/SDTS/`.

### Publishing via GitHub Pages

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) runs
`make build` and deploys `docs/` on every push to `main` that touches
`crates/`, `scenarios/`, `docs/`, `static/`, or the build scripts —
publishing is automatic, nothing to build or commit locally. The repo's
Pages source must be set to **GitHub Actions** (Settings → Pages →
Build and deployment) for the workflow to publish.

The demo is then served at `https://zhagan.github.io/SDTS/`.

### How scenarios are loaded

`scenarios/*.json` (see [Declarative scenarios](scenarios.md)) is the one
source of truth. `scripts/build-docs-scenarios.sh` (run via `make
scenarios`) copies those files into `docs/scenarios/` and generates
`docs/scenarios/manifest.json` — an array of `{file, name, description}`
— so the browser's scenario picker can enumerate what's available without
a backend. Selecting a scenario fetches `docs/scenarios/<file>.json` and
constructs a fresh `SdtsEngine` from its text; the native server instead
reads the same files straight off disk via `Scenario::load`.

### Recordings: export, import, replay

- **Export** — `SdtsEngine.export_recording()` returns the session
  recorded so far as newline-delimited SDTP JSON, byte-for-byte the same
  format the native server writes to `recordings/*.jsonl`. `app.js`
  downloads it as a `.jsonl` file and also saves it into the browser's
  IndexedDB (`opensdts-recordings` database) under **Local Recordings**.
- **Import** — the file input reads a chosen `.jsonl` (or a JSON array of
  envelopes) and hands its text to `SdtsEngine.load_recording()`, which
  validates it before staging it for replay.
- **Replay** — `SdtsEngine.start_replay()` replays whichever recording is
  staged: an explicitly imported one, or otherwise the current session's
  own recording, so pressing **Replay** works immediately after a live run
  with no export/import round trip required. Unlike the native server
  (which loops a replayed file forever with a pause between laps), the
  browser plays a recording once and holds on the final frame — press
  **Replay** again to restart it.
- **Local persistence** — implemented via IndexedDB (see above), with a
  **Clear Local Recordings** button to wipe it. This is a convenience
  layered on top of export/import in `app.js`; the engine itself has no
  concept of browser storage.
