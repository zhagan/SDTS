/// Result of comparing an impact against the target's authoritative
/// position at the moment the impact was received.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitResult {
    pub hit: bool,
    pub distance_mm: f64,
}

/// Pure hit/miss test: distance from the impact to the target center,
/// compared against the target radius. No I/O, no shared state — same
/// inputs always produce the same result, which is what "hits are scored
/// consistently" (docs/mvp.md) requires.
pub fn evaluate(impact_mm: (f64, f64), target_mm: (f64, f64), radius_mm: f64) -> HitResult {
    let dx = impact_mm.0 - target_mm.0;
    let dy = impact_mm.1 - target_mm.1;
    let distance_mm = (dx * dx + dy * dy).sqrt();
    HitResult {
        hit: distance_mm <= radius_mm,
        distance_mm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_center_is_a_hit() {
        let r = evaluate((100.0, 100.0), (100.0, 100.0), 75.0);
        assert!(r.hit);
        assert_eq!(r.distance_mm, 0.0);
    }

    #[test]
    fn exact_boundary_counts_as_a_hit() {
        let r = evaluate((175.0, 100.0), (100.0, 100.0), 75.0);
        assert!(r.hit);
        assert!((r.distance_mm - 75.0).abs() < 1e-9);
    }

    #[test]
    fn just_outside_radius_is_a_miss() {
        let r = evaluate((175.1, 100.0), (100.0, 100.0), 75.0);
        assert!(!r.hit);
    }

    #[test]
    fn distance_matches_3_4_5_triangle() {
        let r = evaluate((3.0, 4.0), (0.0, 0.0), 100.0);
        assert!((r.distance_mm - 5.0).abs() < 1e-9);
    }
}
