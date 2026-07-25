use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const TYPE_SESSION_START: &str = "session_start";
pub const TYPE_TARGET_SPAWN: &str = "target_spawn";
pub const TYPE_TARGET_UPDATE: &str = "target_update";
pub const TYPE_IMPACT: &str = "impact";
pub const TYPE_RESULT: &str = "result";

/// The SDTP envelope: `{ "type", "time", "source", "data" }`.
/// `data` stays untyped JSON so unknown fields are naturally ignored by
/// whichever side doesn't recognize them, per docs/protocol.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub kind: String,
    pub time: f64,
    pub source: String,
    pub data: Value,
}

impl Envelope {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("envelope always serializes")
    }
}

pub fn session_start(
    time: f64,
    session_id: &str,
    arena_width_mm: f64,
    arena_height_mm: f64,
    scenario: &str,
) -> Envelope {
    Envelope {
        kind: TYPE_SESSION_START.to_string(),
        time,
        source: "core".to_string(),
        data: json!({
            "session_id": session_id,
            "arena_width_mm": arena_width_mm,
            "arena_height_mm": arena_height_mm,
            "scenario": scenario,
        }),
    }
}

pub fn target_spawn(
    time: f64,
    target_id: &str,
    shape: &str,
    radius_mm: f64,
    fill: &str,
    stroke: &str,
) -> Envelope {
    Envelope {
        kind: TYPE_TARGET_SPAWN.to_string(),
        time,
        source: "core".to_string(),
        data: json!({
            "target_id": target_id,
            "shape": shape,
            "radius_mm": radius_mm,
            "fill": fill,
            "stroke": stroke,
        }),
    }
}

pub fn target_update(time: f64, target_id: &str, x_mm: f64, y_mm: f64, visible: bool) -> Envelope {
    Envelope {
        kind: TYPE_TARGET_UPDATE.to_string(),
        time,
        source: "core".to_string(),
        data: json!({
            "target_id": target_id,
            "x_mm": x_mm,
            "y_mm": y_mm,
            "visible": visible,
        }),
    }
}

pub fn impact(time: f64, source: &str, impact_id: &str, x_mm: f64, y_mm: f64) -> Envelope {
    Envelope {
        kind: TYPE_IMPACT.to_string(),
        time,
        source: source.to_string(),
        data: json!({
            "impact_id": impact_id,
            "x_mm": x_mm,
            "y_mm": y_mm,
        }),
    }
}

pub fn result(
    time: f64,
    target_id: &str,
    impact_id: &str,
    hit: bool,
    distance_mm: f64,
    impact_x_mm: f64,
    impact_y_mm: f64,
) -> Envelope {
    Envelope {
        kind: TYPE_RESULT.to_string(),
        time,
        source: "core".to_string(),
        data: json!({
            "target_id": target_id,
            "impact_id": impact_id,
            "hit": hit,
            "distance_mm": distance_mm,
            "x_mm": impact_x_mm,
            "y_mm": impact_y_mm,
        }),
    }
}

/// Payload of an incoming `impact` message from a mouse (or other) impact adapter.
#[derive(Debug, Clone, Deserialize)]
pub struct ImpactData {
    pub impact_id: String,
    pub x_mm: f64,
    pub y_mm: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_with_expected_field_names() {
        let env = target_update(1.5, "circle-1", 10.0, 20.0, true);
        let line = env.to_line();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["type"], "target_update");
        assert_eq!(value["time"], 1.5);
        assert_eq!(value["source"], "core");
        assert_eq!(value["data"]["x_mm"], 10.0);
    }

    #[test]
    fn impact_data_deserializes_from_client_payload() {
        let raw = json!({ "impact_id": "abc", "x_mm": 1.0, "y_mm": 2.0 });
        let data: ImpactData = serde_json::from_value(raw).unwrap();
        assert_eq!(data.impact_id, "abc");
        assert_eq!(data.x_mm, 1.0);
    }

    #[test]
    fn result_contains_impact_coordinates() {
        let env = result(1.5, "circle-1", "impact-1", true, 5.0, 110.0, 220.0);
        assert_eq!(env.data["x_mm"], 110.0);
        assert_eq!(env.data["y_mm"], 220.0);
    }
}
