use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::protocol::{self, Envelope, TYPE_RESULT, TYPE_SESSION_START, TYPE_TARGET_SPAWN, TYPE_TARGET_UPDATE};
use crate::scenario::{Scenario, TargetDefinition, TargetState};
use crate::scoring;

/// Errors the engine can report back across the WASM boundary (or to a
/// native caller) as plain strings rather than `io::Error`, since neither
/// browser JS nor `wasm_bindgen::JsError` care about OS error kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EngineError {}

impl From<serde_json::Error> for EngineError {
    fn from(err: serde_json::Error) -> Self {
        EngineError(err.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        EngineError(err.to_string())
    }
}

/// Whether an `Engine` is driving a live scenario simulation or replaying a
/// previously recorded session. Replay never re-runs `Scenario::states_at`
/// — it only plays back the envelopes a live session already recorded, so
/// replay is byte-for-byte reproducible even off a scenario that has since
/// changed on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Live,
    Replay,
}

/// Continuous engine/arena state for rendering. Sent to JS every animation
/// frame; unlike the native server's push-over-websocket model, the WASM
/// engine is polled — the browser calls `snapshot()` (or `snapshot_json()`)
/// once per `requestAnimationFrame` instead of listening for `target_update`
/// messages.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub session_id: String,
    pub scenario_name: String,
    pub scenario_file: String,
    pub mode: &'static str,
    pub running: bool,
    pub finished: bool,
    pub time: f64,
    pub arena_width_mm: f64,
    pub arena_height_mm: f64,
    pub hits: u32,
    pub misses: u32,
    pub targets: Vec<TargetSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetSnapshot {
    pub id: String,
    pub x_mm: f64,
    pub y_mm: f64,
    pub radius_mm: f64,
    pub visible: bool,
    pub shape: String,
    pub fill: String,
    pub stroke: String,
}

/// One scored impact — either the direct return value of `impact()` (live
/// mode) or an entry in the list `replay_update()` returns (replay mode).
/// Kept as a single type since both cases carry the exact same fields as
/// the `result` envelope.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreEvent {
    pub time: f64,
    pub target_id: String,
    pub impact_id: String,
    pub hit: bool,
    pub distance_mm: f64,
    pub x_mm: f64,
    pub y_mm: f64,
    /// `Some(n)` when the hit target has a `durability` budget (n counts
    /// down to 0, at which point it's destroyed and respawns via its own
    /// `sequence`); `None` for an indestructible target or a miss.
    pub hits_remaining: Option<u32>,
}

/// Per-target mutable state the engine tracks alongside the (stateless)
/// `Scenario`: how many hits are left before this target's current
/// appearance is destroyed, its current (possibly shrunk) radius, and a
/// clock offset used to fast-forward a destroyed target past the rest of
/// its current step. `life` is the last-observed `(cycle, step_index)`
/// while visible, used to detect when a new appearance begins so
/// `hits_remaining`/`radius_mm` reset to the target's base values.
#[derive(Debug, Clone)]
struct TargetRuntime {
    time_offset: f64,
    hits_remaining: Option<u32>,
    radius_mm: f64,
    life: (u64, usize),
}

impl TargetRuntime {
    fn fresh(target: &TargetDefinition) -> Self {
        TargetRuntime {
            time_offset: 0.0,
            hits_remaining: target.durability.as_ref().map(|d| d.hits_to_destroy),
            radius_mm: target.display.radius_mm,
            life: (u64::MAX, usize::MAX),
        }
    }
}

/// Evaluates `target`'s state at `global_t` on the engine's live clock,
/// applying its accumulated `time_offset` (so a target the engine just
/// destroyed jumps straight to whatever comes next in its own sequence
/// instead of waiting out the timer). Resets `hits_remaining`/`radius_mm`
/// to the target's base values whenever a new appearance begins, and
/// reports the current (possibly shrunk) radius in place of the
/// scenario's static one.
fn advance_target(
    scenario: &Scenario,
    target: &TargetDefinition,
    runtime: &mut TargetRuntime,
    global_t: f64,
) -> TargetState {
    let local_t = global_t + runtime.time_offset;
    let mut state = scenario.target_state_at(target, local_t);
    let life = (state.cycle, state.step_index);
    if state.visible && runtime.life != life {
        runtime.life = life;
        runtime.hits_remaining = target.durability.as_ref().map(|d| d.hits_to_destroy);
        runtime.radius_mm = target.display.radius_mm;
    }
    state.radius_mm = runtime.radius_mm;
    state
}

/// A non-wall-clock, portable id generator. Real session ids for the native
/// server are wall-clock timestamps (see `sdts-server`'s `recorder` module)
/// so recordings sort chronologically by filename; the engine itself must
/// stay portable to `wasm32-unknown-unknown`, where `SystemTime::now()`
/// panics at runtime, so it falls back to a monotonic in-process counter.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

fn generated_session_id() -> String {
    format!("engine-session-{:06}", NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed))
}

/// The portable OpenSDTS engine: scenario evaluation, impact scoring, SDTP
/// event creation, and session recording, with no dependency on Axum,
/// Tokio, WebSockets, the filesystem, or browser APIs. Runs identically
/// under the native server (via `sdts-server`) and in the browser (via
/// `sdts-wasm`).
pub struct Engine {
    scenario: Scenario,
    scenario_file: String,
    session_id: String,
    mode: Mode,
    running: bool,
    finished: bool,
    time: f64,
    targets: HashMap<String, TargetSnapshot>,
    /// Hit-count/shrink/skip-ahead state per target, keyed by target id.
    /// See `TargetRuntime`/`advance_target`.
    target_runtime: HashMap<String, TargetRuntime>,
    hits: u32,
    misses: u32,
    session_start_envelope: Envelope,
    target_spawn_envelopes: Vec<Envelope>,
    /// Every envelope emitted this session, in emission order — exported
    /// verbatim as the downloadable recording.
    recording: Vec<Envelope>,
    /// Loaded `target_update`/`result` envelopes to play back, sorted by
    /// time, only populated in `Mode::Replay`.
    replay_source: Vec<Envelope>,
    replay_cursor: usize,
}

impl Engine {
    /// Parses and validates `scenario_json`, then builds an engine with an
    /// auto-generated session id. This is the constructor exposed to WASM
    /// (`sdts-wasm`'s `new(scenario_json)`).
    pub fn new(scenario_json: &str) -> Result<Self, EngineError> {
        let scenario: Scenario = serde_json::from_str(scenario_json)?;
        scenario.validate()?;
        Ok(Self::from_scenario(scenario, String::new(), generated_session_id()))
    }

    /// Builds an engine from an already-parsed, already-validated scenario.
    /// Used by `sdts-server`, which loads scenarios from disk (and wants
    /// its own wall-clock session id so recording filenames sort
    /// chronologically).
    pub fn from_scenario(scenario: Scenario, scenario_file: String, session_id: String) -> Self {
        let session_start_envelope = protocol::session_start(
            0.0,
            &session_id,
            scenario.arena.width_mm,
            scenario.arena.height_mm,
            &scenario_file,
        );
        let target_spawn_envelopes: Vec<Envelope> = scenario
            .targets
            .iter()
            .map(|target| {
                protocol::target_spawn(
                    0.0,
                    &target.id,
                    &target.display.shape,
                    target.display.radius_mm,
                    &target.display.fill,
                    &target.display.stroke,
                )
            })
            .collect();
        let targets = scenario
            .targets
            .iter()
            .map(|target| {
                (
                    target.id.clone(),
                    TargetSnapshot {
                        id: target.id.clone(),
                        x_mm: scenario.arena.width_mm / 2.0,
                        y_mm: scenario.arena.height_mm / 2.0,
                        radius_mm: target.display.radius_mm,
                        visible: false,
                        shape: target.display.shape.clone(),
                        fill: target.display.fill.clone(),
                        stroke: target.display.stroke.clone(),
                    },
                )
            })
            .collect();
        let target_runtime = scenario
            .targets
            .iter()
            .map(|target| (target.id.clone(), TargetRuntime::fresh(target)))
            .collect();

        Engine {
            scenario,
            scenario_file,
            session_id,
            mode: Mode::Live,
            running: false,
            finished: false,
            time: 0.0,
            targets,
            target_runtime,
            hits: 0,
            misses: 0,
            session_start_envelope,
            target_spawn_envelopes,
            recording: Vec::new(),
            replay_source: Vec::new(),
            replay_cursor: 0,
        }
    }

    pub fn session_start_envelope(&self) -> &Envelope {
        &self.session_start_envelope
    }

    /// Overrides the scenario file identifier recorded in the
    /// `session_start` envelope's `scenario` field (a bare filename like
    /// `"zigzag.json"`, not `Scenario::name`'s human-readable title — see
    /// `sdts-server`'s use of this same convention). `Engine::new` has no
    /// way to know which file its JSON came from, so the WASM wrapper
    /// calls this right after construction with whatever the browser's
    /// scenario picker selected, purely for recording/display fidelity —
    /// it has no effect on simulation or scoring. Must be called before
    /// `start()`.
    pub fn set_scenario_file(&mut self, scenario_file: impl Into<String>) {
        self.scenario_file = scenario_file.into();
        self.session_start_envelope = protocol::session_start(
            0.0,
            &self.session_id,
            self.scenario.arena.width_mm,
            self.scenario.arena.height_mm,
            &self.scenario_file,
        );
    }

    pub fn target_spawn_envelopes(&self) -> &[Envelope] {
        &self.target_spawn_envelopes
    }

    /// Starts (or restarts) a live scenario session at `time == 0`,
    /// resetting the scoreboard and recording buffer.
    pub fn start(&mut self) {
        self.mode = Mode::Live;
        self.running = true;
        self.finished = false;
        self.time = 0.0;
        self.hits = 0;
        self.misses = 0;
        self.replay_source.clear();
        self.replay_cursor = 0;
        self.recording.clear();
        self.recording.push(self.session_start_envelope.clone());
        self.recording.extend(self.target_spawn_envelopes.clone());
        for target in &self.scenario.targets {
            if let Some(runtime) = self.target_runtime.get_mut(&target.id) {
                *runtime = TargetRuntime::fresh(target);
            }
        }
        for state in self.scenario.states_at(0.0) {
            if let Some(target) = self.targets.get_mut(&state.id) {
                target.x_mm = state.x_mm;
                target.y_mm = state.y_mm;
                target.visible = state.visible;
                target.radius_mm = state.radius_mm;
            }
        }
    }

    /// Freezes the session in place: `update()` and `impact()` become
    /// no-ops until `start()` or `start_replay()` runs again. The last
    /// known target positions and scoreboard remain visible via
    /// `snapshot()`, matching a "Stop" button that pauses rather than
    /// clears the arena.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Advances a live session to `elapsed_seconds` (absolute time since
    /// `start()`, not a frame delta — see docs on why the caller always
    /// passes total elapsed time). Returns the `target_update` envelopes
    /// generated this call, one per target, for callers (like the native
    /// server) that need to broadcast/record them individually.
    pub fn update(&mut self, elapsed_seconds: f64) -> Vec<Envelope> {
        if self.mode != Mode::Live || !self.running {
            return Vec::new();
        }
        self.time = elapsed_seconds.max(0.0);
        let mut envelopes = Vec::with_capacity(self.scenario.targets.len());
        for target in &self.scenario.targets {
            let runtime = self
                .target_runtime
                .get_mut(&target.id)
                .expect("runtime tracked for every scenario target");
            let state = advance_target(&self.scenario, target, runtime, self.time);
            if let Some(snapshot) = self.targets.get_mut(&state.id) {
                snapshot.x_mm = state.x_mm;
                snapshot.y_mm = state.y_mm;
                snapshot.visible = state.visible;
                snapshot.radius_mm = state.radius_mm;
            }
            let env = protocol::target_update(
                self.time,
                &state.id,
                state.x_mm,
                state.y_mm,
                state.visible,
                state.radius_mm,
            );
            self.recording.push(env.clone());
            envelopes.push(env);
        }
        envelopes
    }

    /// Scores an impact against the authoritative target state at
    /// `elapsed_seconds` (recomputed fresh, never the last `update()`
    /// frame's cached position — matching the native server's "score
    /// against Core's position at receipt time" rule). `impact_id` is
    /// supplied by the caller (the impact adapter's concern, not the
    /// Core's) so it can correlate results with its own click events.
    pub fn impact(&mut self, elapsed_seconds: f64, impact_id: &str, x_mm: f64, y_mm: f64) -> ScoreEvent {
        let t = elapsed_seconds.max(0.0);
        if self.mode != Mode::Live || !self.running {
            // Replay is a read-only playback of a past session — there is
            // no live sim to score against, so clicks are a no-op.
            return ScoreEvent {
                time: t,
                target_id: String::new(),
                impact_id: impact_id.to_string(),
                hit: false,
                distance_mm: f64::INFINITY,
                x_mm,
                y_mm,
                hits_remaining: None,
            };
        }
        let impact_env = protocol::impact(t, "mouse", impact_id, x_mm, y_mm);
        self.recording.push(impact_env);

        // (target_id, outcome, seconds left in the target's current step —
        // needed only if this turns out to be the destroying hit).
        let mut best: Option<(String, scoring::HitResult, f64)> = None;
        for target in &self.scenario.targets {
            let runtime = self
                .target_runtime
                .get_mut(&target.id)
                .expect("runtime tracked for every scenario target");
            let state = advance_target(&self.scenario, target, runtime, t);
            if !state.visible {
                continue;
            }
            let outcome = scoring::evaluate((x_mm, y_mm), (state.x_mm, state.y_mm), state.radius_mm);
            let better = best
                .as_ref()
                .map(|(_, current, _)| outcome.distance_mm < current.distance_mm)
                .unwrap_or(true);
            if better {
                best = Some((state.id.clone(), outcome, state.remaining_in_step_secs));
            }
        }

        let (target_id, hit, distance_mm, hits_remaining) = match best {
            Some((id, outcome, remaining_in_step_secs)) if outcome.hit => {
                let hits_remaining = self.apply_hit(&id, remaining_in_step_secs);
                (id, true, outcome.distance_mm, hits_remaining)
            }
            Some((id, outcome, _)) => (id, false, outcome.distance_mm, None),
            None => (String::new(), false, f64::INFINITY, None),
        };
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }

        let result_env = protocol::result(
            t, &target_id, impact_id, hit, distance_mm, x_mm, y_mm, hits_remaining,
        );
        self.recording.push(result_env);

        ScoreEvent {
            time: t,
            target_id,
            impact_id: impact_id.to_string(),
            hit,
            distance_mm,
            x_mm,
            y_mm,
            hits_remaining,
        }
    }

    /// Applies a landed hit to `target_id`'s durability, if it has any
    /// (`None` for an indestructible target, left completely untouched).
    /// On the destroying hit, fast-forwards the target's local clock past
    /// the rest of its current step (`remaining_in_step_secs`) so it
    /// immediately advances to whatever comes next in its own `sequence`
    /// rather than waiting out the timer; otherwise shrinks it toward
    /// `min_radius_factor` of its base radius. Returns the hits remaining
    /// after this hit, for the `result` envelope/`ScoreEvent`.
    fn apply_hit(&mut self, target_id: &str, remaining_in_step_secs: f64) -> Option<u32> {
        let target = self.scenario.targets.iter().find(|t| t.id == target_id)?;
        let durability = target.durability.clone()?;
        let base_radius = target.display.radius_mm;
        let runtime = self.target_runtime.get_mut(target_id)?;
        let remaining = runtime.hits_remaining.unwrap_or(durability.hits_to_destroy);
        if remaining <= 1 {
            runtime.time_offset += remaining_in_step_secs + 1e-6;
            runtime.hits_remaining = Some(0);
            Some(0)
        } else {
            let hits_remaining = remaining - 1;
            runtime.hits_remaining = Some(hits_remaining);
            runtime.radius_mm = (runtime.radius_mm * durability.shrink_per_hit)
                .max(base_radius * durability.min_radius_factor);
            Some(hits_remaining)
        }
    }

    /// Current arena/target/scoreboard state, for rendering.
    pub fn snapshot(&self) -> Snapshot {
        let mut targets: Vec<TargetSnapshot> = self.targets.values().cloned().collect();
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        Snapshot {
            session_id: self.session_id.clone(),
            scenario_name: self.scenario.name.clone(),
            scenario_file: self.scenario_file.clone(),
            mode: match self.mode {
                Mode::Live => "live",
                Mode::Replay => "replay",
            },
            running: self.running,
            finished: self.finished,
            time: self.time,
            arena_width_mm: self.scenario.arena.width_mm,
            arena_height_mm: self.scenario.arena.height_mm,
            hits: self.hits,
            misses: self.misses,
            targets,
        }
    }

    /// Serializes every envelope emitted so far as newline-delimited JSON
    /// (one envelope per line), matching the native server's on-disk
    /// `recordings/<id>.jsonl` format exactly, so files exported from the
    /// browser and files recorded natively are interchangeable.
    pub fn export_recording(&self) -> String {
        self.recording
            .iter()
            .map(Envelope::to_line)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parses a recording as either newline-delimited JSON (the native
    /// on-disk format) or a single JSON array of envelopes (in case a user
    /// hand-edits or re-exports one as an array). Ignores blank lines and
    /// lines that don't parse as an envelope, so trailing newlines or
    /// stray whitespace in an imported file don't cause a hard failure.
    pub fn parse_recording(recording_text: &str) -> Result<Vec<Envelope>, EngineError> {
        let trimmed = recording_text.trim_start();
        if trimmed.starts_with('[') {
            let envelopes: Vec<Envelope> = serde_json::from_str(trimmed)?;
            return Ok(envelopes);
        }

        let envelopes: Vec<Envelope> = recording_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<Envelope>(line).ok())
            .collect();
        if envelopes.is_empty() {
            return Err(EngineError("recording contains no valid SDTP envelopes".to_string()));
        }
        Ok(envelopes)
    }

    /// Switches the engine into replay mode, driven entirely by
    /// `replay_update()` — there is no background timer (WASM has none to
    /// give it), so playback only advances when the caller passes a new
    /// `elapsed_seconds`. Unlike the native server's replay (which loops
    /// a file forever with a pause between laps), the browser demo plays a
    /// recording once and holds on the final frame; call `start_replay`
    /// again to restart it.
    pub fn start_replay(&mut self, recording_text: &str) -> Result<(), EngineError> {
        let envelopes = Self::parse_recording(recording_text)?;

        let session_start = envelopes
            .iter()
            .find(|e| e.kind == TYPE_SESSION_START)
            .cloned()
            .unwrap_or_else(|| protocol::session_start(0.0, "replay", 1600.0, 1000.0, "replay"));
        let spawns: Vec<Envelope> = envelopes
            .iter()
            .filter(|e| e.kind == TYPE_TARGET_SPAWN)
            .cloned()
            .collect();
        let mut playback: Vec<Envelope> = envelopes
            .into_iter()
            .filter(|e| e.kind == TYPE_TARGET_UPDATE || e.kind == TYPE_RESULT)
            .collect();
        playback.sort_by(|a, b| a.time.total_cmp(&b.time));

        let session_id = session_start.data["session_id"].as_str().unwrap_or("replay").to_string();
        let arena_width_mm = session_start.data["arena_width_mm"].as_f64().unwrap_or(1600.0);
        let arena_height_mm = session_start.data["arena_height_mm"].as_f64().unwrap_or(1000.0);
        let scenario_file = session_start.data["scenario"].as_str().unwrap_or("replay").to_string();

        self.scenario.arena.width_mm = arena_width_mm;
        self.scenario.arena.height_mm = arena_height_mm;
        self.scenario_file = scenario_file;
        self.session_id = session_id;
        self.session_start_envelope = session_start;
        self.target_spawn_envelopes = spawns.clone();

        self.targets = spawns
            .iter()
            .map(|env| {
                let id = env.data["target_id"].as_str().unwrap_or("").to_string();
                (
                    id.clone(),
                    TargetSnapshot {
                        id,
                        x_mm: arena_width_mm / 2.0,
                        y_mm: arena_height_mm / 2.0,
                        radius_mm: env.data["radius_mm"].as_f64().unwrap_or(0.0),
                        visible: false,
                        shape: env.data["shape"].as_str().unwrap_or("circle").to_string(),
                        fill: env.data["fill"].as_str().unwrap_or("#e3b341").to_string(),
                        stroke: env.data["stroke"].as_str().unwrap_or("#c99a2e").to_string(),
                    },
                )
            })
            .collect();

        self.mode = Mode::Replay;
        self.running = true;
        self.finished = playback.is_empty();
        self.time = 0.0;
        self.hits = 0;
        self.misses = 0;
        self.replay_cursor = 0;
        self.recording = std::iter::once(self.session_start_envelope.clone())
            .chain(self.target_spawn_envelopes.clone())
            .chain(playback.clone())
            .collect();
        self.replay_source = playback;
        Ok(())
    }

    /// Advances replay to `elapsed_seconds`, applying every recorded
    /// envelope whose time has now passed, and returns the `result` events
    /// crossed this call (for the browser to flash hit/miss markers — the
    /// same information `impact()` returns directly in live mode).
    pub fn replay_update(&mut self, elapsed_seconds: f64) -> Vec<ScoreEvent> {
        if self.mode != Mode::Replay || !self.running {
            return Vec::new();
        }
        let t = elapsed_seconds.max(0.0);
        self.time = t;
        let mut results = Vec::new();
        while self.replay_cursor < self.replay_source.len()
            && self.replay_source[self.replay_cursor].time <= t
        {
            let env = self.replay_source[self.replay_cursor].clone();
            self.replay_cursor += 1;
            match env.kind.as_str() {
                k if k == TYPE_TARGET_UPDATE => {
                    let id = env.data["target_id"].as_str().unwrap_or("").to_string();
                    if let Some(target) = self.targets.get_mut(&id) {
                        target.x_mm = env.data["x_mm"].as_f64().unwrap_or(target.x_mm);
                        target.y_mm = env.data["y_mm"].as_f64().unwrap_or(target.y_mm);
                        target.visible = env.data["visible"].as_bool().unwrap_or(target.visible);
                        target.radius_mm = env.data["radius_mm"].as_f64().unwrap_or(target.radius_mm);
                    }
                }
                k if k == TYPE_RESULT => {
                    let hit = env.data["hit"].as_bool().unwrap_or(false);
                    if hit {
                        self.hits += 1;
                    } else {
                        self.misses += 1;
                    }
                    results.push(ScoreEvent {
                        time: env.time,
                        target_id: env.data["target_id"].as_str().unwrap_or("").to_string(),
                        impact_id: env.data["impact_id"].as_str().unwrap_or("").to_string(),
                        hit,
                        distance_mm: env.data["distance_mm"].as_f64().unwrap_or(f64::INFINITY),
                        x_mm: env.data["x_mm"].as_f64().unwrap_or(0.0),
                        y_mm: env.data["y_mm"].as_f64().unwrap_or(0.0),
                        hits_remaining: env.data["hits_remaining"].as_u64().map(|v| v as u32),
                    });
                }
                _ => {}
            }
        }
        if self.replay_cursor >= self.replay_source.len() {
            self.finished = true;
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scenario with one target that sits still at (100, 100) with a
    /// 50mm radius for a long time — deterministic and time-independent,
    /// so impact/scoring tests don't need to chase a moving target.
    fn stationary_scenario_json() -> &'static str {
        r##"{
            "name": "Stationary Test Target",
            "seed": 1,
            "arena": { "width_mm": 800, "height_mm": 600 },
            "targets": [
                {
                    "id": "circle-1",
                    "display": { "shape": "circle", "radius_mm": 50, "fill": "#fff", "stroke": "#000" },
                    "repeat": false,
                    "sequence": [
                        {
                            "action": "show",
                            "duration_secs": 1000,
                            "position": { "type": "fixed", "x_mm": 100, "y_mm": 100 }
                        }
                    ]
                }
            ]
        }"##
    }

    /// A single stationary target with a 3-hit durability budget: hits 1
    /// and 2 shrink it (50% per hit, floored at 20% of its 50mm base
    /// radius), and hit 3 destroys it. The show step's duration (1000s) is
    /// deliberately huge relative to any `t` used in tests, so the
    /// destroying hit's clock-skip always lands in the *next* cycle
    /// (same fixed position, since `position.type == "fixed"` doesn't vary
    /// by cycle) — a clean, deterministic "respawned" state to assert on
    /// without needing to advance real time.
    fn destructible_scenario_json() -> &'static str {
        r##"{
            "name": "Destructible Test Target",
            "seed": 1,
            "arena": { "width_mm": 800, "height_mm": 600 },
            "targets": [
                {
                    "id": "circle-1",
                    "display": { "shape": "circle", "radius_mm": 50, "fill": "#fff", "stroke": "#000" },
                    "repeat": true,
                    "durability": { "hits_to_destroy": 3, "shrink_per_hit": 0.5, "min_radius_factor": 0.2 },
                    "sequence": [
                        {
                            "action": "show",
                            "duration_secs": 1000,
                            "position": { "type": "fixed", "x_mm": 100, "y_mm": 100 }
                        }
                    ]
                }
            ]
        }"##
    }

    /// Two independent stationary targets, both destructible, far enough
    /// apart that an impact can only ever be closest to one of them.
    fn two_destructible_targets_scenario_json() -> &'static str {
        r##"{
            "name": "Two Destructible Targets",
            "seed": 1,
            "arena": { "width_mm": 800, "height_mm": 600 },
            "targets": [
                {
                    "id": "left",
                    "display": { "shape": "circle", "radius_mm": 50, "fill": "#fff", "stroke": "#000" },
                    "repeat": true,
                    "durability": { "hits_to_destroy": 2, "shrink_per_hit": 0.5, "min_radius_factor": 0.2 },
                    "sequence": [
                        { "action": "show", "duration_secs": 1000, "position": { "type": "fixed", "x_mm": 100, "y_mm": 100 } }
                    ]
                },
                {
                    "id": "right",
                    "display": { "shape": "circle", "radius_mm": 50, "fill": "#fff", "stroke": "#000" },
                    "repeat": true,
                    "durability": { "hits_to_destroy": 2, "shrink_per_hit": 0.5, "min_radius_factor": 0.2 },
                    "sequence": [
                        { "action": "show", "duration_secs": 1000, "position": { "type": "fixed", "x_mm": 700, "y_mm": 500 } }
                    ]
                }
            ]
        }"##
    }

    #[test]
    fn new_rejects_invalid_scenario_json() {
        assert!(Engine::new("{ not json").is_err());
        assert!(Engine::new(r#"{"name":"bad","arena":{"width_mm":0,"height_mm":1},"targets":[]}"#).is_err());
    }

    #[test]
    fn start_emits_session_start_and_spawn_into_recording() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.start();
        let snapshot = engine.snapshot();
        assert!(snapshot.running);
        assert_eq!(snapshot.mode, "live");
        assert_eq!(snapshot.targets.len(), 1);

        let recording = engine.export_recording();
        assert!(recording.contains("session_start"));
        assert!(recording.contains("target_spawn"));
    }

    #[test]
    fn update_moves_time_forward_and_reflects_target_state() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.start();
        let envelopes = engine.update(2.5);
        assert_eq!(envelopes.len(), 1);
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.time, 2.5);
        assert!(snapshot.targets[0].visible);
        assert_eq!(snapshot.targets[0].x_mm, 100.0);
        assert_eq!(snapshot.targets[0].y_mm, 100.0);
    }

    #[test]
    fn impact_scores_hit_and_miss_against_authoritative_state() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.start();
        engine.update(1.0);

        let hit = engine.impact(1.0, "impact-1", 100.0, 100.0);
        assert!(hit.hit);
        assert_eq!(hit.target_id, "circle-1");

        let miss = engine.impact(1.0, "impact-2", 900.0, 900.0);
        assert!(!miss.hit);

        let snapshot = engine.snapshot();
        assert_eq!(snapshot.hits, 1);
        assert_eq!(snapshot.misses, 1);
    }

    #[test]
    fn hits_shrink_a_destructible_target_and_destroy_it_on_the_final_hit() {
        let mut engine = Engine::new(destructible_scenario_json()).unwrap();
        engine.start();

        let first = engine.impact(1.0, "impact-1", 100.0, 100.0);
        assert!(first.hit);
        assert_eq!(first.hits_remaining, Some(2));
        engine.update(1.0);
        assert_eq!(engine.snapshot().targets[0].radius_mm, 25.0);

        let second = engine.impact(1.0, "impact-2", 100.0, 100.0);
        assert!(second.hit);
        assert_eq!(second.hits_remaining, Some(1));
        engine.update(1.0);
        assert_eq!(engine.snapshot().targets[0].radius_mm, 12.5);

        // Destroying hit: hits_remaining reaches 0 and the target's clock
        // skips ahead into its next appearance.
        let third = engine.impact(1.0, "impact-3", 100.0, 100.0);
        assert!(third.hit);
        assert_eq!(third.hits_remaining, Some(0));

        // The next impact lands on a fresh appearance: full size, full
        // durability restored, same fixed position.
        let fourth = engine.impact(1.0, "impact-4", 100.0, 100.0);
        assert!(fourth.hit);
        assert_eq!(fourth.hits_remaining, Some(2));
        engine.update(1.0);
        assert_eq!(engine.snapshot().targets[0].radius_mm, 25.0);
    }

    #[test]
    fn indestructible_targets_ignore_hits_remaining_and_never_shrink() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.start();
        for i in 0..5 {
            let outcome = engine.impact(1.0, &format!("impact-{i}"), 100.0, 100.0);
            assert!(outcome.hit);
            assert_eq!(outcome.hits_remaining, None);
        }
        assert_eq!(engine.snapshot().targets[0].radius_mm, 50.0);
    }

    #[test]
    fn destroying_one_of_two_simultaneous_targets_leaves_the_other_untouched() {
        let mut engine = Engine::new(two_destructible_targets_scenario_json()).unwrap();
        engine.start();

        // Destroy "left" (2 hits) without ever touching "right".
        let hit1 = engine.impact(1.0, "impact-1", 100.0, 100.0);
        assert_eq!(hit1.target_id, "left");
        assert_eq!(hit1.hits_remaining, Some(1));
        let hit2 = engine.impact(1.0, "impact-2", 100.0, 100.0);
        assert_eq!(hit2.target_id, "left");
        assert_eq!(hit2.hits_remaining, Some(0));

        engine.update(1.0);
        let snapshot = engine.snapshot();
        let right = snapshot.targets.iter().find(|t| t.id == "right").unwrap();
        assert_eq!(right.radius_mm, 50.0, "untouched target keeps its base radius");

        // "right" still takes its own full 2 hits to destroy.
        let hit3 = engine.impact(1.0, "impact-3", 700.0, 500.0);
        assert_eq!(hit3.target_id, "right");
        assert_eq!(hit3.hits_remaining, Some(1));
    }

    #[test]
    fn export_then_load_recording_round_trips() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.start();
        engine.update(1.0);
        engine.impact(1.0, "impact-1", 100.0, 100.0);
        engine.update(2.0);

        let exported = engine.export_recording();
        let parsed = Engine::parse_recording(&exported).unwrap();
        assert!(parsed.iter().any(|e| e.kind == "session_start"));
        assert!(parsed.iter().any(|e| e.kind == "target_spawn"));
        assert!(parsed.iter().any(|e| e.kind == "impact"));
        assert!(parsed.iter().any(|e| e.kind == "result"));
        assert!(parsed.iter().filter(|e| e.kind == "target_update").count() >= 2);
    }

    #[test]
    fn replay_reproduces_recorded_hits_and_final_state() {
        let mut live = Engine::new(stationary_scenario_json()).unwrap();
        live.start();
        live.update(1.0);
        let outcome = live.impact(1.0, "impact-1", 100.0, 100.0);
        assert!(outcome.hit);
        live.update(2.0);
        let recording = live.export_recording();

        let mut replay = Engine::new(stationary_scenario_json()).unwrap();
        replay.start_replay(&recording).unwrap();
        assert_eq!(replay.snapshot().mode, "replay");
        assert!(!replay.snapshot().finished);

        let results_early = replay.replay_update(1.0);
        assert_eq!(results_early.len(), 1);
        assert!(results_early[0].hit);
        assert_eq!(replay.snapshot().hits, 1);

        let results_late = replay.replay_update(10.0);
        assert!(results_late.is_empty());
        assert!(replay.snapshot().finished);
        assert_eq!(replay.snapshot().targets[0].x_mm, 100.0);
    }

    #[test]
    fn load_recording_rejects_garbage() {
        assert!(Engine::parse_recording("not an envelope at all").is_err());
    }

    #[test]
    fn set_scenario_file_is_reflected_in_recorded_session_start() {
        let mut engine = Engine::new(stationary_scenario_json()).unwrap();
        engine.set_scenario_file("zigzag.json");
        engine.start();
        assert_eq!(engine.session_start_envelope().data["scenario"], "zigzag.json");
        assert_eq!(engine.snapshot().scenario_file, "zigzag.json");
    }

    #[test]
    fn impact_is_a_no_op_during_replay() {
        let mut live = Engine::new(stationary_scenario_json()).unwrap();
        live.start();
        live.update(1.0);
        live.impact(1.0, "impact-1", 100.0, 100.0);
        let recording = live.export_recording();

        let mut replay = Engine::new(stationary_scenario_json()).unwrap();
        replay.start_replay(&recording).unwrap();
        let outcome = replay.impact(1.0, "impact-2", 100.0, 100.0);
        assert!(!outcome.hit);
        assert_eq!(replay.snapshot().hits, 0);
        assert_eq!(replay.snapshot().misses, 0);
    }
}
