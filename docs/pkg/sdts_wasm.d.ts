/* tslint:disable */
/* eslint-disable */
export class SdtsEngine {
  free(): void;
  constructor(scenario_json: string);
  /**
   * Records which scenario file this session's JSON came from, purely
   * for recording/display fidelity (see `Engine::set_scenario_file`).
   * Call before `start()`.
   */
  set_scenario_file(scenario_file: string): void;
  start(): void;
  stop(): void;
  update(elapsed_seconds: number): void;
  /**
   * Scores a click/tap at arena millimeter coordinates and returns the
   * resulting `ScoreEvent` as a JSON string (`{ hit, distance_mm,
   * target_id, x_mm, y_mm, time, impact_id }`).
   */
  impact(elapsed_seconds: number, x_mm: number, y_mm: number): string;
  /**
   * Current arena/target/scoreboard state as a JSON string, for
   * rendering — see `Snapshot` in `sdts-engine` for the exact shape.
   */
  snapshot_json(): string;
  /**
   * The full session recording so far, as newline-delimited SDTP JSON —
   * ready to be downloaded as a `.jsonl` file.
   */
  export_recording(): string;
  /**
   * Validates and stages an imported recording (NDJSON or a JSON array
   * of envelopes) for `start_replay`, without switching modes yet.
   */
  load_recording(recording_json: string): void;
  /**
   * Starts replaying whichever recording is staged: an explicitly
   * imported one (`load_recording`), or otherwise the current session's
   * own recording so far (`export_recording`) — so "Replay" works
   * immediately after a live run without requiring an export/import
   * round trip.
   */
  start_replay(): void;
  /**
   * Advances replay to `elapsed_seconds` and returns the `result` events
   * crossed this call as a JSON array (for hit/miss marker animation) —
   * empty (`"[]"`) most frames.
   */
  replay_update(elapsed_seconds: number): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_sdtsengine_free: (a: number, b: number) => void;
  readonly sdtsengine_new: (a: number, b: number) => [number, number, number];
  readonly sdtsengine_set_scenario_file: (a: number, b: number, c: number) => void;
  readonly sdtsengine_start: (a: number) => void;
  readonly sdtsengine_stop: (a: number) => void;
  readonly sdtsengine_update: (a: number, b: number) => void;
  readonly sdtsengine_impact: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly sdtsengine_snapshot_json: (a: number) => [number, number, number, number];
  readonly sdtsengine_export_recording: (a: number) => [number, number];
  readonly sdtsengine_load_recording: (a: number, b: number, c: number) => [number, number];
  readonly sdtsengine_start_replay: (a: number) => [number, number];
  readonly sdtsengine_replay_update: (a: number, b: number) => [number, number, number, number];
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_3: WebAssembly.Table;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
