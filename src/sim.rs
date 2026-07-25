/// Deterministic circle-target simulation.
///
/// Position is a pure function of elapsed session time: no RNG, no
/// integrated/accumulated state. That's what makes "circle moves
/// deterministically" true (docs/mvp.md) — the same `t` always yields the
/// same `(x, y)`, in this process or any other.
#[derive(Debug, Clone, Copy)]
pub struct Simulation {
    pub arena_width_mm: f64,
    pub arena_height_mm: f64,
    pub radius_mm: f64,
    amplitude_x: f64,
    amplitude_y: f64,
    period_x_secs: f64,
    period_y_secs: f64,
}

impl Simulation {
    pub fn new() -> Self {
        let arena_width_mm = 1600.0;
        let arena_height_mm = 1000.0;
        Simulation {
            arena_width_mm,
            arena_height_mm,
            radius_mm: 75.0,
            amplitude_x: arena_width_mm / 2.0 - 100.0,
            amplitude_y: arena_height_mm / 2.0 - 100.0,
            period_x_secs: 16.0,
            period_y_secs: 72.0,
        }
    }

    /// Zig-zag path: a fast horizontal triangle wave (the side-to-side
    /// "zig-zag") crossed with a slower vertical one, so the target
    /// sweeps back and forth while traversing top-to-bottom.
    pub fn position_at(&self, t: f64) -> (f64, f64) {
        let cx =
            self.arena_width_mm / 2.0 + self.amplitude_x * triangle_wave(t, self.period_x_secs);
        let cy =
            self.arena_height_mm / 2.0 + self.amplitude_y * triangle_wave(t, self.period_y_secs);
        (cx, cy)
    }
}

impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}

/// Triangle wave oscillating in `[-1, 1]` with the given period, starting
/// at `-1` and rising linearly to `1` at the half period before falling
/// back — a pure function of `t`, so playback stays exactly reproducible.
fn triangle_wave(t: f64, period_secs: f64) -> f64 {
    let phase = (t / period_secs).rem_euclid(1.0);
    if phase < 0.5 {
        4.0 * phase - 1.0
    } else {
        3.0 - 4.0 * phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_is_pure_and_reproducible() {
        let sim = Simulation::new();
        assert_eq!(sim.position_at(3.7), sim.position_at(3.7));
        assert_eq!(sim.position_at(0.0), sim.position_at(0.0));
    }

    #[test]
    fn triangle_wave_starts_at_minimum_and_bounces_between_extremes() {
        assert_eq!(triangle_wave(0.0, 2.0), -1.0);
        assert!((triangle_wave(0.5, 2.0) - 0.0).abs() < 1e-9);
        assert!((triangle_wave(1.0, 2.0) - 1.0).abs() < 1e-9);
        assert!((triangle_wave(1.5, 2.0) - 0.0).abs() < 1e-9);
        assert!((triangle_wave(2.0, 2.0) - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn position_stays_within_arena_amplitude_bounds() {
        let sim = Simulation::new();
        for i in 0..1000 {
            let t = i as f64 * 0.037;
            let (x, y) = sim.position_at(t);
            assert!(x >= sim.arena_width_mm / 2.0 - sim.amplitude_x - 1e-9);
            assert!(x <= sim.arena_width_mm / 2.0 + sim.amplitude_x + 1e-9);
            assert!(y >= sim.arena_height_mm / 2.0 - sim.amplitude_y - 1e-9);
            assert!(y <= sim.arena_height_mm / 2.0 + sim.amplitude_y + 1e-9);
        }
    }
}
