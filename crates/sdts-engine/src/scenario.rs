use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;

use serde::{Deserialize, Serialize};

pub const SCENARIOS_DIR: &str = "scenarios";
pub const DEFAULT_SCENARIO: &str = "zigzag.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_seed")]
    pub seed: u64,
    pub arena: Arena,
    pub targets: Vec<TargetDefinition>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Arena {
    pub width_mm: f64,
    pub height_mm: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetDefinition {
    pub id: String,
    pub display: DisplayDefinition,
    #[serde(default = "default_true")]
    pub repeat: bool,
    pub sequence: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DisplayDefinition {
    #[serde(default = "default_shape")]
    pub shape: String,
    pub radius_mm: f64,
    #[serde(default = "default_fill")]
    pub fill: String,
    #[serde(default = "default_stroke")]
    pub stroke: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    Show {
        duration_secs: DurationSpec,
        #[serde(flatten)]
        motion: Motion,
    },
    Hide {
        duration_secs: DurationSpec,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DurationSpec {
    Fixed(f64),
    Random { random: Range },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Motion {
    Position { position: PositionSpec },
    Path { path: PathSpec },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PositionSpec {
    Fixed {
        x_mm: f64,
        y_mm: f64,
    },
    Random {
        #[serde(default)]
        margin_mm: f64,
        #[serde(default)]
        minimum_distance_mm: f64,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathSpec {
    Line {
        from: Point,
        to: Point,
    },
    Polyline {
        points: Vec<Point>,
    },
    Zigzag {
        from: Point,
        to: Point,
        #[serde(default = "default_zigzags")]
        zigzags: u32,
    },
    RandomLine {
        #[serde(default)]
        margin_mm: f64,
        distance_mm: Range,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Point {
    pub x_mm: f64,
    pub y_mm: f64,
}

#[derive(Debug, Clone)]
pub struct TargetState {
    pub id: String,
    pub x_mm: f64,
    pub y_mm: f64,
    pub radius_mm: f64,
    pub visible: bool,
}

impl Scenario {
    pub fn load(name: &str) -> io::Result<Self> {
        let safe_name = safe_scenario_name(name)?;
        let contents = std::fs::read_to_string(format!("{SCENARIOS_DIR}/{safe_name}"))?;
        let scenario: Scenario = serde_json::from_str(&contents)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.arena.width_mm <= 0.0 || self.arena.height_mm <= 0.0 {
            return invalid("arena dimensions must be positive");
        }
        if self.targets.is_empty() {
            return invalid("a scenario must define at least one target");
        }
        for target in &self.targets {
            if target.id.is_empty() || target.sequence.is_empty() || target.display.radius_mm <= 0.0
            {
                return invalid("targets require an id, positive radius, and non-empty sequence");
            }
            for step in &target.sequence {
                let duration = match step {
                    Step::Show {
                        duration_secs,
                        motion,
                    } => {
                        validate_motion(motion)?;
                        duration_secs
                    }
                    Step::Hide { duration_secs } => duration_secs,
                };
                match duration {
                    DurationSpec::Fixed(value) if *value <= 0.0 => {
                        return invalid("step durations must be positive")
                    }
                    DurationSpec::Random { random }
                        if random.min <= 0.0 || random.max < random.min =>
                    {
                        return invalid("random durations require 0 < min <= max")
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn states_at(&self, t: f64) -> Vec<TargetState> {
        self.targets
            .iter()
            .map(|target| self.target_state_at(target, t.max(0.0)))
            .collect()
    }

    fn target_state_at(&self, target: &TargetDefinition, mut remaining: f64) -> TargetState {
        let mut cycle = 0_u64;
        let mut previous = Point {
            x_mm: self.arena.width_mm / 2.0,
            y_mm: self.arena.height_mm / 2.0,
        };

        loop {
            for (step_index, step) in target.sequence.iter().enumerate() {
                let duration =
                    sampled_duration(step.duration(), self.seed, &target.id, cycle, step_index);
                let point = match step {
                    Step::Show { motion, .. } => motion.point_at(
                        if duration > 0.0 {
                            (remaining / duration).clamp(0.0, 1.0)
                        } else {
                            0.0
                        },
                        self,
                        target,
                        cycle,
                        step_index,
                        previous,
                    ),
                    Step::Hide { .. } => previous,
                };

                if remaining < duration {
                    return TargetState {
                        id: target.id.clone(),
                        x_mm: point.x_mm,
                        y_mm: point.y_mm,
                        radius_mm: target.display.radius_mm,
                        visible: matches!(step, Step::Show { .. }),
                    };
                }
                remaining -= duration;
                if matches!(step, Step::Show { .. }) {
                    previous = motion_end_point(step, self, target, cycle, step_index, previous);
                }
            }
            if !target.repeat {
                return TargetState {
                    id: target.id.clone(),
                    x_mm: previous.x_mm,
                    y_mm: previous.y_mm,
                    radius_mm: target.display.radius_mm,
                    visible: false,
                };
            }
            cycle += 1;
        }
    }
}

impl Step {
    fn duration(&self) -> &DurationSpec {
        match self {
            Step::Show { duration_secs, .. } | Step::Hide { duration_secs } => duration_secs,
        }
    }
}

impl Motion {
    fn point_at(
        &self,
        progress: f64,
        scenario: &Scenario,
        target: &TargetDefinition,
        cycle: u64,
        step_index: usize,
        previous: Point,
    ) -> Point {
        match self {
            Motion::Position { position } => {
                position.resolve(scenario, target, cycle, step_index, previous)
            }
            Motion::Path { path } => {
                path.point_at(progress, scenario, target, cycle, step_index, previous)
            }
        }
    }
}

impl PositionSpec {
    fn resolve(
        &self,
        scenario: &Scenario,
        target: &TargetDefinition,
        cycle: u64,
        step_index: usize,
        previous: Point,
    ) -> Point {
        match self {
            PositionSpec::Fixed { x_mm, y_mm } => Point {
                x_mm: *x_mm,
                y_mm: *y_mm,
            },
            PositionSpec::Random {
                margin_mm,
                minimum_distance_mm,
            } => {
                let inset = target.display.radius_mm + margin_mm;
                let min_x = inset.min(scenario.arena.width_mm / 2.0);
                let max_x = (scenario.arena.width_mm - inset).max(min_x);
                let min_y = inset.min(scenario.arena.height_mm / 2.0);
                let max_y = (scenario.arena.height_mm - inset).max(min_y);
                let mut candidate = previous;
                for attempt in 0..32 {
                    candidate = Point {
                        x_mm: lerp(
                            min_x,
                            max_x,
                            random_unit(scenario.seed, &target.id, cycle, step_index, attempt, 0),
                        ),
                        y_mm: lerp(
                            min_y,
                            max_y,
                            random_unit(scenario.seed, &target.id, cycle, step_index, attempt, 1),
                        ),
                    };
                    if distance(candidate, previous) >= *minimum_distance_mm {
                        break;
                    }
                }
                candidate
            }
        }
    }
}

impl PathSpec {
    fn point_at(
        &self,
        progress: f64,
        scenario: &Scenario,
        target: &TargetDefinition,
        cycle: u64,
        step_index: usize,
        previous: Point,
    ) -> Point {
        match self {
            PathSpec::Line { from, to } => interpolate(*from, *to, progress),
            PathSpec::Polyline { points } => point_on_polyline(points, progress),
            PathSpec::Zigzag { from, to, zigzags } => {
                let count = (*zigzags).max(1) as f64;
                let base = interpolate(*from, *to, progress);
                let direction = if (progress * count).floor() as u32 % 2 == 0 {
                    1.0
                } else {
                    -1.0
                };
                let offset = ((progress * count).fract() * 2.0 - 1.0) * direction;
                Point {
                    x_mm: lerp(from.x_mm, to.x_mm, (offset + 1.0) / 2.0),
                    y_mm: base.y_mm,
                }
            }
            PathSpec::RandomLine {
                margin_mm,
                distance_mm,
            } => {
                let start = PositionSpec::Random {
                    margin_mm: *margin_mm,
                    minimum_distance_mm: 0.0,
                }
                .resolve(scenario, target, cycle, step_index, previous);
                let angle = random_unit(scenario.seed, &target.id, cycle, step_index, 0, 21)
                    * std::f64::consts::TAU;
                let distance = lerp(
                    distance_mm.min,
                    distance_mm.max,
                    random_unit(scenario.seed, &target.id, cycle, step_index, 0, 22),
                );
                let inset = target.display.radius_mm + margin_mm;
                let end = Point {
                    x_mm: (start.x_mm + angle.cos() * distance)
                        .clamp(inset, scenario.arena.width_mm - inset),
                    y_mm: (start.y_mm + angle.sin() * distance)
                        .clamp(inset, scenario.arena.height_mm - inset),
                };
                interpolate(start, end, progress)
            }
        }
    }
}

fn motion_end_point(
    step: &Step,
    scenario: &Scenario,
    target: &TargetDefinition,
    cycle: u64,
    step_index: usize,
    previous: Point,
) -> Point {
    match step {
        Step::Show { motion, .. } => {
            motion.point_at(1.0, scenario, target, cycle, step_index, previous)
        }
        Step::Hide { .. } => previous,
    }
}

fn sampled_duration(spec: &DurationSpec, seed: u64, target: &str, cycle: u64, step: usize) -> f64 {
    match spec {
        DurationSpec::Fixed(value) => *value,
        DurationSpec::Random { random } => lerp(
            random.min,
            random.max,
            random_unit(seed, target, cycle, step, 0, 9),
        ),
    }
}

fn random_unit(seed: u64, target: &str, cycle: u64, step: usize, attempt: usize, axis: u8) -> f64 {
    let mut hasher = DefaultHasher::new();
    (seed, target, cycle, step, attempt, axis).hash(&mut hasher);
    hasher.finish() as f64 / u64::MAX as f64
}

fn point_on_polyline(points: &[Point], progress: f64) -> Point {
    if points.is_empty() {
        return Point {
            x_mm: 0.0,
            y_mm: 0.0,
        };
    }
    if points.len() == 1 {
        return points[0];
    }
    let scaled = progress.clamp(0.0, 1.0) * (points.len() - 1) as f64;
    let segment = (scaled.floor() as usize).min(points.len() - 2);
    interpolate(
        points[segment],
        points[segment + 1],
        scaled - segment as f64,
    )
}

fn interpolate(from: Point, to: Point, amount: f64) -> Point {
    Point {
        x_mm: lerp(from.x_mm, to.x_mm, amount),
        y_mm: lerp(from.y_mm, to.y_mm, amount),
    }
}

fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

fn distance(a: Point, b: Point) -> f64 {
    ((a.x_mm - b.x_mm).powi(2) + (a.y_mm - b.y_mm).powi(2)).sqrt()
}

fn validate_motion(motion: &Motion) -> io::Result<()> {
    if let Motion::Path { path } = motion {
        match path {
            PathSpec::Polyline { points } if points.len() < 2 => {
                return invalid("polyline paths require at least two points");
            }
            PathSpec::RandomLine { distance_mm, .. }
                if distance_mm.min <= 0.0 || distance_mm.max < distance_mm.min =>
            {
                return invalid("random-line distance requires 0 < min <= max");
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn safe_scenario_name(name: &str) -> io::Result<&str> {
    let path = std::path::Path::new(name);
    if name.is_empty()
        || path.file_name().and_then(|value| value.to_str()) != Some(name)
        || !name.ends_with(".json")
    {
        return invalid("invalid scenario filename");
    }
    Ok(name)
}

fn invalid<T>(message: &str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn default_seed() -> u64 {
    1
}
fn default_true() -> bool {
    true
}
fn default_shape() -> String {
    "circle".to_string()
}
fn default_fill() -> String {
    "#e3b341".to_string()
}
fn default_stroke() -> String {
    "#c99a2e".to_string()
}
fn default_zigzags() -> u32 {
    6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup() -> Scenario {
        serde_json::from_str(include_str!("../../../scenarios/random-popup.json")).unwrap()
    }

    #[test]
    fn random_popup_is_deterministic_and_hides_between_appearances() {
        let scenario = popup();
        let first = scenario.states_at(1.0);
        assert!(first[0].visible);
        assert_eq!(first[0].x_mm, scenario.states_at(1.0)[0].x_mm);
        assert!(!scenario.states_at(3.5)[0].visible);
    }

    #[test]
    fn line_interpolates_at_half_time() {
        let path = PathSpec::Line {
            from: Point {
                x_mm: 0.0,
                y_mm: 10.0,
            },
            to: Point {
                x_mm: 100.0,
                y_mm: 30.0,
            },
        };
        let scenario = popup();
        let point = path.point_at(
            0.5,
            &scenario,
            &scenario.targets[0],
            0,
            0,
            Point {
                x_mm: 0.0,
                y_mm: 0.0,
            },
        );
        assert_eq!(point.x_mm, 50.0);
        assert_eq!(point.y_mm, 20.0);
    }

    #[test]
    fn multiple_targets_are_evaluated_independently() {
        let scenario: Scenario =
            serde_json::from_str(include_str!("../../../scenarios/crossing-pair.json")).unwrap();
        let states = scenario.states_at(4.0);
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|target| target.visible));
        assert_eq!(states[0].x_mm, 800.0);
        assert_eq!(states[1].x_mm, 800.0);
    }

    #[test]
    fn random_positions_stay_inside_the_arena() {
        let scenario = popup();
        let target = &scenario.targets[0];
        for second in 0..120 {
            let state = &scenario.states_at(second as f64)[0];
            if state.visible {
                assert!(state.x_mm >= target.display.radius_mm);
                assert!(state.x_mm <= scenario.arena.width_mm - target.display.radius_mm);
                assert!(state.y_mm >= target.display.radius_mm);
                assert!(state.y_mm <= scenario.arena.height_mm - target.display.radius_mm);
            }
        }
    }

    #[test]
    fn random_line_is_deterministic_and_stays_inside_the_arena() {
        let scenario: Scenario =
            serde_json::from_str(include_str!("../../../scenarios/random-dash.json")).unwrap();
        let first = &scenario.states_at(1.5)[0];
        let again = &scenario.states_at(1.5)[0];
        assert_eq!(first.x_mm, again.x_mm);
        assert_eq!(first.y_mm, again.y_mm);
        assert!(first.visible);
        assert!(first.x_mm >= first.radius_mm);
        assert!(first.x_mm <= scenario.arena.width_mm - first.radius_mm);
        assert!(first.y_mm >= first.radius_mm);
        assert!(first.y_mm <= scenario.arena.height_mm - first.radius_mm);
        assert!(!scenario.states_at(3.5)[0].visible);
    }
}
