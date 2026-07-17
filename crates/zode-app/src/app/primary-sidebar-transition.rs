use std::time::Instant;

use zode_app_ui::{
    PrimarySidebarLayoutOptions, SecondarySidebarLayoutOptions, SidebarLayoutOptions,
};

use super::{sidebar_motion, DesktopApp};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TransitionAdvance {
    pub layout_changed: bool,
    pub needs_frame: bool,
}

/// Host-owned visibility for the persistent primary sidebar.
///
/// Product state records the final user choice immediately. This transition
/// keeps the outgoing sidebar geometry alive until a close animation reaches
/// zero, while a reversed command continues from the current progress.
#[derive(Debug, Clone)]
pub(super) struct PrimarySidebarTransition {
    progress: f32,
    target_open: bool,
    last_advanced_at: Option<Instant>,
}

impl PrimarySidebarTransition {
    pub fn seeded(open: bool) -> Self {
        Self {
            progress: if open { 1.0 } else { 0.0 },
            target_open: open,
            last_advanced_at: None,
        }
    }

    pub fn set_target(&mut self, open: bool, reduced_motion: bool) -> bool {
        let previous_target = self.target_open;
        let previous_progress = self.progress;
        if self.target_open != open {
            self.target_open = open;
            self.last_advanced_at = None;
        }
        if reduced_motion {
            self.progress = if open { 1.0 } else { 0.0 };
            self.last_advanced_at = None;
        }
        previous_target != self.target_open || previous_progress != self.progress
    }

    pub fn adopt_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
        self.last_advanced_at = None;
    }

    pub fn advance(&mut self, now: Instant) -> TransitionAdvance {
        let target = if self.target_open { 1.0 } else { 0.0 };
        if self.progress == target {
            self.last_advanced_at = None;
            return TransitionAdvance::default();
        }

        let Some(previous) = self.last_advanced_at.replace(now) else {
            return TransitionAdvance {
                layout_changed: false,
                needs_frame: true,
            };
        };
        let duration = if self.target_open {
            sidebar_motion::OPEN_DURATION
        } else {
            sidebar_motion::CLOSE_DURATION
        };
        let elapsed = now.checked_duration_since(previous).unwrap_or_default();
        let delta = elapsed.as_secs_f32() / duration.as_secs_f32();
        let previous_progress = self.progress;
        if self.target_open {
            self.progress = (self.progress + delta).min(1.0);
        } else {
            self.progress = (self.progress - delta).max(0.0);
        }
        let needs_frame = self.progress != target;
        if !needs_frame {
            self.last_advanced_at = None;
        }
        TransitionAdvance {
            layout_changed: self.progress != previous_progress,
            needs_frame,
        }
    }

    pub fn visibility(&self) -> f32 {
        sidebar_motion::eased_visibility(self.progress)
    }

    pub fn is_present(&self) -> bool {
        self.target_open || self.progress > 0.0
    }

    pub fn is_active(&self) -> bool {
        self.progress != if self.target_open { 1.0 } else { 0.0 }
    }
}

impl DesktopApp {
    pub(super) fn sync_primary_sidebar_transition(&mut self) -> bool {
        if self.app_state.shell.sidebar_open && !self.primary_sidebar_transition.is_present() {
            self.primary_sidebar_transition
                .adopt_progress(self.primary_sidebar_preview.progress());
        }
        self.primary_sidebar_transition.set_target(
            self.app_state.shell.sidebar_open,
            self.app_state.ui_preferences.reduced_motion,
        )
    }

    pub(super) fn advance_primary_sidebar_transition(&mut self, now: Instant) -> TransitionAdvance {
        self.primary_sidebar_transition.advance(now)
    }

    pub(super) fn primary_sidebar_transition_is_active(&self) -> bool {
        self.primary_sidebar_transition.is_active()
    }

    pub(super) fn primary_sidebar_transition_is_present(&self) -> bool {
        self.primary_sidebar_transition.is_present()
    }

    pub(super) fn animated_sidebar_layout_options(&self) -> SidebarLayoutOptions {
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: self.primary_sidebar_transition.is_present(),
                width: f32::from(self.app_state.ui_preferences.primary_sidebar_width),
                visibility: self.primary_sidebar_transition.visibility(),
            },
            secondary: SecondarySidebarLayoutOptions {
                open: self.right_secondary_sidebar_is_present(),
                width: f32::from(self.app_state.ui_preferences.secondary_sidebar_width),
                pinned_summary_overlay: self.right_pinned_summary_mode()
                    == zode_app_ui::PinnedSummaryMode::Overlay,
                visibility: self.right_secondary_sidebar_visibility(),
                pinned_summary_visibility: if self.right_pinned_summary_is_present() {
                    self.right_pinned_summary_visibility()
                } else {
                    0.0
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_1,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn seed_matches_the_persisted_target_without_startup_animation() {
        let open = PrimarySidebarTransition::seeded(true);
        let closed = PrimarySidebarTransition::seeded(false);

        assert_near(open.visibility(), 1.0);
        assert!(open.is_present());
        assert!(!open.is_active());
        assert_near(closed.visibility(), 0.0);
        assert!(!closed.is_present());
        assert!(!closed.is_active());
    }

    #[test]
    fn close_keeps_the_surface_present_until_progress_reaches_zero() {
        let mut transition = PrimarySidebarTransition::seeded(true);
        let start = Instant::now();

        assert!(transition.set_target(false, false));
        assert!(transition.is_present());
        assert!(transition.advance(start).needs_frame);
        let halfway = transition.advance(start + sidebar_motion::CLOSE_DURATION / 2);
        assert!(halfway.layout_changed);
        assert!(halfway.needs_frame);
        assert!(transition.visibility() > 0.0);
        let finished = transition.advance(start + sidebar_motion::CLOSE_DURATION);
        assert!(finished.layout_changed);
        assert!(!finished.needs_frame);
        assert!(!transition.is_present());
    }

    #[test]
    fn reversing_mid_transition_continues_from_the_current_progress() {
        let mut transition = PrimarySidebarTransition::seeded(true);
        let start = Instant::now();

        transition.set_target(false, false);
        transition.advance(start);
        transition.advance(start + sidebar_motion::CLOSE_DURATION / 2);
        let closing_visibility = transition.visibility();
        assert!(closing_visibility > 0.0 && closing_visibility < 1.0);

        transition.set_target(true, false);
        assert_near(transition.visibility(), closing_visibility);
        assert!(
            transition
                .advance(start + sidebar_motion::CLOSE_DURATION / 2)
                .needs_frame
        );
        transition.advance(
            start + sidebar_motion::CLOSE_DURATION / 2 + sidebar_motion::OPEN_DURATION / 2,
        );
        assert!(transition.visibility() > closing_visibility);
    }

    #[test]
    fn sixty_hertz_open_has_enough_monotonic_frames_to_read_as_motion() {
        let mut transition = PrimarySidebarTransition::seeded(false);
        transition.set_target(true, false);
        let start = Instant::now();
        transition.advance(start);
        let mut previous = transition.visibility();
        let mut moving_frames = 0;
        for frame in 1..=20 {
            let advance = transition.advance(start + Duration::from_millis(frame * 16));
            let current = transition.visibility();
            if current > previous {
                moving_frames += 1;
            }
            assert!(current >= previous);
            previous = current;
            if !advance.needs_frame {
                break;
            }
        }
        assert!(moving_frames >= 10, "only {moving_frames} moving frames");
        assert_near(transition.visibility(), 1.0);
    }

    #[test]
    fn reduced_motion_snaps_to_each_new_target() {
        let mut transition = PrimarySidebarTransition::seeded(true);

        transition.set_target(false, true);
        assert_near(transition.visibility(), 0.0);
        assert!(!transition.is_present());
        assert!(!transition.is_active());

        transition.set_target(true, true);
        assert_near(transition.visibility(), 1.0);
        assert!(transition.is_present());
        assert!(!transition.is_active());
    }
}
