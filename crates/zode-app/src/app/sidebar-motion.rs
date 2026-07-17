use std::time::Duration;

/// Persistent sidebars should feel responsive without reading as a snap.
/// These durations keep a 60 Hz transition above ten visible frames while
/// remaining shorter than a menu or route transition.
pub(super) const OPEN_DURATION: Duration = Duration::from_millis(220);
pub(super) const CLOSE_DURATION: Duration = Duration::from_millis(180);

/// A reversible S curve with a small linear component.
///
/// Pure smoothstep has zero velocity at both ends. At the old 120/160 ms
/// durations that looked like a pause followed by a jump. Blending in a small
/// linear share keeps the first and last frames moving, while the smoothstep
/// share still removes the mechanical linear feel. The mapping is independent
/// of direction, so reversing a transition never changes the visible value.
pub(super) fn eased_visibility(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let smoothstep = progress * progress * (3.0 - 2.0 * progress);
    0.32 * progress + 0.68 * smoothstep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_1,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn curve_preserves_endpoints_and_midpoint() {
        assert_near(eased_visibility(0.0), 0.0);
        assert_near(eased_visibility(0.5), 0.5);
        assert_near(eased_visibility(1.0), 1.0);
    }

    #[test]
    fn curve_is_strictly_monotonic_at_animation_frame_steps() {
        let mut previous = eased_visibility(0.0);
        for frame in 1..=14 {
            let current = eased_visibility(frame as f32 / 14.0);
            assert!(current > previous, "frame {frame} did not advance");
            previous = current;
        }
    }

    #[test]
    fn curve_keeps_visible_motion_at_both_edges() {
        assert!(eased_visibility(0.05) > 0.01);
        assert!(eased_visibility(0.95) < 0.99);
    }
}
