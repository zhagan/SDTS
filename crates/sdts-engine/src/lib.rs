//! Portable OpenSDTS domain logic: scenario parsing/validation, seeded
//! deterministic scenario evaluation, impact scoring, SDTP event creation,
//! and session recording/replay. No dependency on Axum, Tokio, WebSockets,
//! the filesystem, or browser APIs — compiles and runs identically for
//! native Rust (`sdts-server`) and `wasm32-unknown-unknown` (`sdts-wasm`).

pub mod engine;
pub mod protocol;
pub mod scenario;
pub mod scoring;

pub use engine::{Engine, EngineError, ScoreEvent, Snapshot, TargetSnapshot};
pub use protocol::Envelope;
pub use scenario::Scenario;
