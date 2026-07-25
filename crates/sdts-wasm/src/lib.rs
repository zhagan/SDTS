//! Thin `wasm-bindgen` wrapper around `sdts_engine::Engine`. This crate
//! contains no domain logic of its own — every method here just calls
//! straight into the engine and (de)serializes JSON across the JS/WASM
//! boundary, so the browser runs exactly the same scenario/scoring/replay
//! code as the native server, never a JS reimplementation of it.

use std::sync::atomic::{AtomicU64, Ordering};

use sdts_engine::Engine;
use wasm_bindgen::prelude::*;

/// Impact ids are the impact adapter's concern, not the Core's (see
/// `Engine::impact`'s doc comment) — the mouse/pointer handler in `app.js`
/// is the adapter here, but giving it a UUID dependency just to tag clicks
/// would be needless ceremony, so the WASM wrapper mints a simple
/// monotonic id on its behalf.
static NEXT_IMPACT_ID: AtomicU64 = AtomicU64::new(1);

fn next_impact_id() -> String {
    format!("impact-{:08}", NEXT_IMPACT_ID.fetch_add(1, Ordering::Relaxed))
}

#[wasm_bindgen]
pub struct SdtsEngine {
    inner: Engine,
    /// Set by `load_recording`, consumed by `start_replay`. Lets the
    /// browser either "Replay" an explicitly imported file or, if nothing
    /// was imported, replay the current session's own `export_recording`.
    pending_recording: Option<String>,
}

#[wasm_bindgen]
impl SdtsEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(scenario_json: &str) -> Result<SdtsEngine, JsError> {
        console_error_panic_hook::set_once();
        let inner = Engine::new(scenario_json).map_err(|err| JsError::new(&err.to_string()))?;
        Ok(SdtsEngine {
            inner,
            pending_recording: None,
        })
    }

    /// Records which scenario file this session's JSON came from, purely
    /// for recording/display fidelity (see `Engine::set_scenario_file`).
    /// Call before `start()`.
    pub fn set_scenario_file(&mut self, scenario_file: &str) {
        self.inner.set_scenario_file(scenario_file);
    }

    pub fn start(&mut self) {
        self.inner.start();
    }

    pub fn stop(&mut self) {
        self.inner.stop();
    }

    pub fn update(&mut self, elapsed_seconds: f64) {
        self.inner.update(elapsed_seconds);
    }

    /// Scores a click/tap at arena millimeter coordinates and returns the
    /// resulting `ScoreEvent` as a JSON string (`{ hit, distance_mm,
    /// target_id, x_mm, y_mm, time, impact_id }`).
    pub fn impact(&mut self, elapsed_seconds: f64, x_mm: f64, y_mm: f64) -> Result<String, JsError> {
        let impact_id = next_impact_id();
        let outcome = self.inner.impact(elapsed_seconds, &impact_id, x_mm, y_mm);
        serde_json::to_string(&outcome).map_err(|err| JsError::new(&err.to_string()))
    }

    /// Current arena/target/scoreboard state as a JSON string, for
    /// rendering — see `Snapshot` in `sdts-engine` for the exact shape.
    pub fn snapshot_json(&self) -> Result<String, JsError> {
        serde_json::to_string(&self.inner.snapshot()).map_err(|err| JsError::new(&err.to_string()))
    }

    /// The full session recording so far, as newline-delimited SDTP JSON —
    /// ready to be downloaded as a `.jsonl` file.
    pub fn export_recording(&self) -> String {
        self.inner.export_recording()
    }

    /// Validates and stages an imported recording (NDJSON or a JSON array
    /// of envelopes) for `start_replay`, without switching modes yet.
    pub fn load_recording(&mut self, recording_json: &str) -> Result<(), JsError> {
        Engine::parse_recording(recording_json).map_err(|err| JsError::new(&err.to_string()))?;
        self.pending_recording = Some(recording_json.to_string());
        Ok(())
    }

    /// Starts replaying whichever recording is staged: an explicitly
    /// imported one (`load_recording`), or otherwise the current session's
    /// own recording so far (`export_recording`) — so "Replay" works
    /// immediately after a live run without requiring an export/import
    /// round trip.
    pub fn start_replay(&mut self) -> Result<(), JsError> {
        let text = self
            .pending_recording
            .take()
            .unwrap_or_else(|| self.inner.export_recording());
        self.inner
            .start_replay(&text)
            .map_err(|err| JsError::new(&err.to_string()))
    }

    /// Advances replay to `elapsed_seconds` and returns the `result` events
    /// crossed this call as a JSON array (for hit/miss marker animation) —
    /// empty (`"[]"`) most frames.
    pub fn replay_update(&mut self, elapsed_seconds: f64) -> Result<String, JsError> {
        let results = self.inner.replay_update(elapsed_seconds);
        serde_json::to_string(&results).map_err(|err| JsError::new(&err.to_string()))
    }
}
