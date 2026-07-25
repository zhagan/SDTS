// OpenSDTS browser demo. All scenario evaluation, scoring, recording, and
// replay logic lives in the WASM engine (crates/sdts-wasm wrapping
// crates/sdts-engine) — this file only does canvas rendering, pointer
// input, and local recording persistence. There is no JavaScript
// reimplementation of any engine behavior here.
import init, { SdtsEngine } from "./pkg/sdts_wasm.js";

const canvas = document.getElementById("arena");
const ctx = canvas.getContext("2d");
const arenaWrap = document.getElementById("arenaWrap");
const statusEl = document.getElementById("status");
const scenarioSelect = document.getElementById("scenarioSelect");
const scenarioDescriptionEl = document.getElementById("scenarioDescription");
const startBtn = document.getElementById("startBtn");
const stopBtn = document.getElementById("stopBtn");
const replayBtn = document.getElementById("replayBtn");
const exportBtn = document.getElementById("exportBtn");
const importInput = document.getElementById("importInput");
const clearLocalBtn = document.getElementById("clearLocalBtn");
const localRecordingListEl = document.getElementById("localRecordingList");
const timeTextEl = document.getElementById("timeText");
const hitCountEl = document.getElementById("hitCount");
const missCountEl = document.getElementById("missCount");
const scoreTextEl = document.getElementById("scoreText");
const hintEl = document.getElementById("hint");

let wasmEngine = null;
let mode = "idle"; // idle | live | replay
let startedAt = 0;
let lastSnapshot = null;
let arenaWidthMm = 1600;
let arenaHeightMm = 1000;
let scale = 1;
let markers = []; // { xMm, yMm, hit, addedAt }
let flashes = []; // { hit, addedAt }
let scenarioManifest = [];
const scenarioTextCache = new Map();

function setStatus(text, cls) {
  statusEl.textContent = text;
  statusEl.className = cls;
}

// ---------------------------------------------------------------------
// Scenario loading — scenarios/manifest.json is generated at build time
// (scripts/build-docs-scenarios.sh) from the same scenarios/*.json files
// the native server reads, so both share one source of truth.
// ---------------------------------------------------------------------

async function loadScenarioManifest() {
  const res = await fetch("scenarios/manifest.json");
  if (!res.ok) throw new Error(`failed to load scenario manifest (status ${res.status})`);
  scenarioManifest = await res.json();
  scenarioSelect.innerHTML = "";
  for (const entry of scenarioManifest) {
    const option = document.createElement("option");
    option.value = entry.file;
    option.textContent = entry.name;
    scenarioSelect.appendChild(option);
  }
  updateScenarioDescription();
}

function updateScenarioDescription() {
  const entry = scenarioManifest.find((item) => item.file === scenarioSelect.value);
  scenarioDescriptionEl.textContent = entry ? entry.description : "";
}

async function fetchScenarioText(file) {
  if (scenarioTextCache.has(file)) return scenarioTextCache.get(file);
  const res = await fetch(`scenarios/${file}`);
  if (!res.ok) throw new Error(`failed to load scenario ${file} (status ${res.status})`);
  const text = await res.text();
  scenarioTextCache.set(file, text);
  return text;
}

/// Lazily (re)builds the WASM engine for whichever scenario is currently
/// selected. A fresh `SdtsEngine` instance owns exactly one scenario, so
/// switching scenarios means constructing a new one — cheap, since it's
/// just parsing a small JSON file.
async function ensureEngine() {
  if (wasmEngine) return;
  const file = scenarioSelect.value;
  if (!file) throw new Error("no scenario selected");
  const text = await fetchScenarioText(file);
  wasmEngine = new SdtsEngine(text);
  wasmEngine.set_scenario_file(file);
  lastSnapshot = JSON.parse(wasmEngine.snapshot_json());
}

scenarioSelect.addEventListener("change", () => {
  updateScenarioDescription();
  // A new scenario needs a new engine instance — drop the old one and let
  // the next Start/Replay press build it lazily.
  wasmEngine = null;
  mode = "idle";
});

// ---------------------------------------------------------------------
// Rendering — Rust returns authoritative arena/target state in millimeters
// via snapshot_json(); this file only maps mm -> canvas pixels.
// ---------------------------------------------------------------------

function resizeCanvas() {
  const cssWidth = Math.min(arenaWrap.clientWidth, 900);
  const cssHeight = cssWidth * (arenaHeightMm / arenaWidthMm);
  const dpr = window.devicePixelRatio || 1;
  canvas.style.width = `${cssWidth}px`;
  canvas.style.height = `${cssHeight}px`;
  canvas.width = Math.round(cssWidth * dpr);
  canvas.height = Math.round(cssHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  scale = cssWidth / arenaWidthMm;
}
window.addEventListener("resize", resizeCanvas);

function pushMarker(scoreEvent) {
  const now = performance.now();
  markers.push({ xMm: scoreEvent.x_mm, yMm: scoreEvent.y_mm, hit: scoreEvent.hit, addedAt: now });
  flashes.push({ hit: scoreEvent.hit, addedAt: now });
}

function draw(snapshot) {
  if (snapshot.arena_width_mm !== arenaWidthMm || snapshot.arena_height_mm !== arenaHeightMm) {
    arenaWidthMm = snapshot.arena_width_mm;
    arenaHeightMm = snapshot.arena_height_mm;
    resizeCanvas();
  }
  const dpr = window.devicePixelRatio || 1;
  const cssWidth = canvas.width / dpr;
  const cssHeight = canvas.height / dpr;
  ctx.clearRect(0, 0, cssWidth, cssHeight);

  // Hidden targets are never drawn (and, in the engine, never scored).
  for (const target of snapshot.targets) {
    if (!target.visible || target.radius_mm <= 0) continue;
    const tx = target.x_mm * scale;
    const ty = target.y_mm * scale;
    ctx.beginPath();
    ctx.arc(tx, ty, target.radius_mm * scale, 0, Math.PI * 2);
    ctx.fillStyle = target.fill;
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = target.stroke;
    ctx.stroke();
  }

  const now = performance.now();
  markers = markers.filter((m) => now - m.addedAt < 1200);
  for (const m of markers) {
    const mx = m.xMm * scale;
    const my = m.yMm * scale;
    const age = (now - m.addedAt) / 1200;
    ctx.globalAlpha = 1 - age;
    ctx.beginPath();
    ctx.arc(mx, my, 8, 0, Math.PI * 2);
    ctx.strokeStyle = m.hit ? "#4caf7d" : "#d9534f";
    ctx.lineWidth = 3;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  flashes = flashes.filter((f) => now - f.addedAt < 800);
  const flash = flashes[flashes.length - 1];
  if (flash) {
    const age = (now - flash.addedAt) / 800;
    ctx.globalAlpha = 1 - age;
    ctx.font = "bold 48px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillStyle = flash.hit ? "#4caf7d" : "#d9534f";
    ctx.fillText(flash.hit ? "HIT" : "MISS", cssWidth / 2, cssHeight / 2);
    ctx.globalAlpha = 1;
  }
}

function updateStats(snapshot) {
  timeTextEl.textContent = `${snapshot.time.toFixed(1)}s`;
  hitCountEl.textContent = snapshot.hits;
  missCountEl.textContent = snapshot.misses;
  const total = snapshot.hits + snapshot.misses;
  const pct = total > 0 ? ` (${Math.round((100 * snapshot.hits) / total)}%)` : "";
  scoreTextEl.textContent = `${snapshot.hits}/${total}${pct}`;
}

function updateStatusFromSnapshot(snapshot) {
  if (snapshot.mode === "replay") {
    if (snapshot.finished) {
      setStatus(`replay finished — ${snapshot.scenario_file || snapshot.scenario_name}`, "stopped");
    } else {
      setStatus(snapshot.running ? "replaying…" : "replay paused", snapshot.running ? "running" : "stopped");
    }
  } else {
    setStatus(
      snapshot.running ? `recording live — ${snapshot.scenario_name}` : "stopped",
      snapshot.running ? "running" : "stopped",
    );
  }
  stopBtn.disabled = !snapshot.running;
  canvas.classList.toggle("idle", snapshot.mode !== "live" || !snapshot.running);
  hintEl.textContent =
    snapshot.mode === "replay"
      ? "Replay mode — clicks are ignored during playback."
      : "Click the target — green ring = hit, red ring = miss.";
}

function frame() {
  if (wasmEngine) {
    try {
      const elapsed = Math.max(0, (performance.now() - startedAt) / 1000);
      if (mode === "live") {
        wasmEngine.update(elapsed);
      } else if (mode === "replay") {
        const results = JSON.parse(wasmEngine.replay_update(elapsed));
        for (const result of results) pushMarker(result);
      }
      lastSnapshot = JSON.parse(wasmEngine.snapshot_json());
      draw(lastSnapshot);
      updateStats(lastSnapshot);
      updateStatusFromSnapshot(lastSnapshot);
    } catch (err) {
      console.error(err);
      setStatus(`engine error: ${err.message || err}`, "error");
    }
  }
  requestAnimationFrame(frame);
}

canvas.addEventListener("click", (e) => {
  if (!wasmEngine || mode !== "live" || !lastSnapshot || !lastSnapshot.running) return;
  const rect = canvas.getBoundingClientRect();
  const xMm = (e.clientX - rect.left) / scale;
  const yMm = (e.clientY - rect.top) / scale;
  const elapsed = Math.max(0, (performance.now() - startedAt) / 1000);
  try {
    const outcome = JSON.parse(wasmEngine.impact(elapsed, xMm, yMm));
    pushMarker(outcome);
  } catch (err) {
    console.error(err);
    setStatus(`engine error: ${err.message || err}`, "error");
  }
});

// ---------------------------------------------------------------------
// Start / Stop / Replay / Export / Import
// ---------------------------------------------------------------------

startBtn.addEventListener("click", async () => {
  try {
    await ensureEngine();
    wasmEngine.start();
    mode = "live";
    startedAt = performance.now();
    markers = [];
    flashes = [];
    replayBtn.disabled = false;
    exportBtn.disabled = false;
  } catch (err) {
    console.error(err);
    setStatus(`failed to start: ${err.message || err}`, "error");
  }
});

stopBtn.addEventListener("click", () => {
  if (wasmEngine) wasmEngine.stop();
});

replayBtn.addEventListener("click", async () => {
  try {
    await ensureEngine();
    wasmEngine.start_replay();
    mode = "replay";
    startedAt = performance.now();
    markers = [];
    flashes = [];
  } catch (err) {
    console.error(err);
    setStatus(`replay failed: ${err.message || err}`, "error");
  }
});

function timestampForFilename() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function downloadText(filename, text) {
  const blob = new Blob([text], { type: "application/x-ndjson" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

exportBtn.addEventListener("click", async () => {
  if (!wasmEngine) return;
  const text = wasmEngine.export_recording();
  if (!text) {
    setStatus("nothing to export yet — press Start first", "error");
    return;
  }
  const name = `sdts-recording-${timestampForFilename()}.jsonl`;
  downloadText(name, text);
  try {
    await saveLocalRecording(name, text);
    await refreshLocalRecordings();
  } catch (err) {
    console.error("failed to save recording locally", err);
  }
});

importInput.addEventListener("change", async () => {
  const file = importInput.files[0];
  if (!file) return;
  try {
    const text = await file.text();
    await ensureEngine();
    wasmEngine.load_recording(text);
    replayBtn.disabled = false;
    setStatus(`imported ${file.name} — press Replay to play it back`, "ready");
    await saveLocalRecording(file.name, text);
    await refreshLocalRecordings();
  } catch (err) {
    console.error(err);
    setStatus(`import failed: ${err.message || err}`, "error");
  } finally {
    importInput.value = "";
  }
});

// ---------------------------------------------------------------------
// Local recording persistence (IndexedDB). This is a convenience on top
// of the engine's own export/import — the engine has no idea local
// storage exists; it just hands JS a recording string either way.
// ---------------------------------------------------------------------

const DB_NAME = "opensdts-recordings";
const STORE_NAME = "recordings";

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: "name" });
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function saveLocalRecording(name, text) {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).put({ name, text, size: text.length, savedAt: Date.now() });
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function listLocalRecordings() {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const req = tx.objectStore(STORE_NAME).getAll();
    req.onsuccess = () => resolve(req.result.sort((a, b) => b.savedAt - a.savedAt));
    req.onerror = () => reject(req.error);
  });
}

async function deleteLocalRecording(name) {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).delete(name);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function clearLocalRecordings() {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).clear();
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function refreshLocalRecordings() {
  const recordings = await listLocalRecordings();
  if (recordings.length === 0) {
    localRecordingListEl.innerHTML =
      '<span id="recordingEmpty">No local recordings yet — export one to save it here.</span>';
    return;
  }
  localRecordingListEl.innerHTML = "";
  for (const rec of recordings) {
    const row = document.createElement("div");
    row.className = "recording-row";

    const label = document.createElement("span");
    const date = new Date(rec.savedAt).toLocaleString();
    label.textContent = `${rec.name} (${(rec.size / 1024).toFixed(1)} KB) — ${date}`;

    const playBtn = document.createElement("button");
    playBtn.textContent = "▶ Replay";
    playBtn.addEventListener("click", async () => {
      try {
        await ensureEngine();
        wasmEngine.load_recording(rec.text);
        wasmEngine.start_replay();
        mode = "replay";
        startedAt = performance.now();
        markers = [];
        flashes = [];
      } catch (err) {
        console.error(err);
        setStatus(`replay failed: ${err.message || err}`, "error");
      }
    });

    const deleteBtn = document.createElement("button");
    deleteBtn.className = "danger";
    deleteBtn.textContent = "Delete";
    deleteBtn.addEventListener("click", async () => {
      await deleteLocalRecording(rec.name);
      await refreshLocalRecordings();
    });

    const actions = document.createElement("div");
    actions.className = "recording-actions";
    actions.appendChild(playBtn);
    actions.appendChild(deleteBtn);

    row.appendChild(label);
    row.appendChild(actions);
    localRecordingListEl.appendChild(row);
  }
}

clearLocalBtn.addEventListener("click", async () => {
  if (!window.confirm("Delete all locally saved recordings? This cannot be undone.")) return;
  await clearLocalRecordings();
  await refreshLocalRecordings();
});

// ---------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------

async function main() {
  setStatus("loading engine…", "ready");
  await init();
  await loadScenarioManifest();
  resizeCanvas();
  scenarioSelect.disabled = false;
  startBtn.disabled = false;
  setStatus("engine ready — select a scenario and press Start", "ready");
  await refreshLocalRecordings();
  requestAnimationFrame(frame);
}

main().catch((err) => {
  console.error(err);
  setStatus(`failed to initialize: ${err.message || err}`, "error");
});
