use std::time::Instant;

use zode_app_model::{ShellRoute, ZodeAppState};
use zode_app_ui::{PinnedSummaryMode, SECONDARY_PANE_BREAKPOINT};

use super::{sidebar_motion, DesktopApp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RightPanelTargets {
    pub secondary_open: bool,
    pub summary: PinnedSummaryMode,
}

impl Default for RightPanelTargets {
    fn default() -> Self {
        Self {
            secondary_open: false,
            summary: PinnedSummaryMode::Hidden,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TransitionAdvance {
    pub layout_changed: bool,
    pub needs_frame: bool,
}

/// Host-owned visibility for the two independent right-side surfaces.
///
/// Product state records the user's final choice. This state deliberately
/// survives a close command long enough to keep the outgoing geometry in the
/// immutable layout snapshot until the transition reaches zero.
#[derive(Debug, Clone)]
pub(super) struct RightPanelTransition {
    secondary: VisibilityTransition,
    summary: VisibilityTransition,
    summary_mode: PinnedSummaryMode,
    initialized: bool,
}

impl Default for RightPanelTransition {
    fn default() -> Self {
        Self {
            secondary: VisibilityTransition::default(),
            summary: VisibilityTransition::default(),
            summary_mode: PinnedSummaryMode::Hidden,
            initialized: false,
        }
    }
}

impl RightPanelTransition {
    pub fn seeded(targets: RightPanelTargets) -> Self {
        let mut transition = Self::default();
        transition.sync_targets(targets, true);
        transition
    }

    pub fn sync_targets(&mut self, targets: RightPanelTargets, reduced_motion: bool) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.secondary.seed(targets.secondary_open);
            self.summary
                .seed(targets.summary != PinnedSummaryMode::Hidden);
            self.summary_mode = targets.summary;
            return true;
        }

        let mut changed = self
            .secondary
            .set_target(targets.secondary_open, reduced_motion);
        let summary_open = targets.summary != PinnedSummaryMode::Hidden;
        if summary_open {
            changed |= self.summary_mode != targets.summary;
            self.summary_mode = targets.summary;
        }
        changed |= self.summary.set_target(summary_open, reduced_motion);
        if !self.summary.is_present() {
            self.summary_mode = PinnedSummaryMode::Hidden;
        }
        changed
    }

    pub fn advance(&mut self, now: Instant) -> TransitionAdvance {
        let secondary = self.secondary.advance(now);
        let summary = self.summary.advance(now);
        if !self.summary.is_present() {
            self.summary_mode = PinnedSummaryMode::Hidden;
        }
        TransitionAdvance {
            layout_changed: secondary.layout_changed || summary.layout_changed,
            needs_frame: secondary.needs_frame || summary.needs_frame,
        }
    }

    pub fn secondary_visibility(&self) -> f32 {
        self.secondary.eased_progress()
    }

    pub fn secondary_is_present(&self) -> bool {
        self.secondary.is_present()
    }

    pub fn summary_visibility(&self) -> f32 {
        self.summary.eased_progress()
    }

    pub fn summary_is_present(&self) -> bool {
        self.summary.is_present()
    }

    pub fn summary_mode(&self) -> PinnedSummaryMode {
        self.summary_mode
    }
}

pub(super) fn targets_for_state(state: &ZodeAppState, viewport_width: f32) -> RightPanelTargets {
    let summary = if state.presentation.route != ShellRoute::Conversation {
        PinnedSummaryMode::Hidden
    } else if state.presentation.pinned_summary_overlay_open {
        PinnedSummaryMode::Overlay
    } else if state.current_session.is_some()
        && !state.presentation.pinned_summary_auto_hidden
        && !state.presentation.secondary_sidebar_open
        && viewport_width >= SECONDARY_PANE_BREAKPOINT
    {
        PinnedSummaryMode::Docked
    } else {
        PinnedSummaryMode::Hidden
    };
    RightPanelTargets {
        secondary_open: state.presentation.route == ShellRoute::Conversation
            && state.presentation.secondary_sidebar_open,
        summary,
    }
}

impl DesktopApp {
    pub(super) fn sync_right_panel_transition(&mut self) -> bool {
        let (width, _) = self.window_state.logical_size();
        self.right_panel_transition.sync_targets(
            targets_for_state(&self.app_state, width),
            self.app_state.ui_preferences.reduced_motion,
        )
    }

    /// Responsive presentation changes are geometry fallbacks, not user
    /// commands. Snap them so crossing the split breakpoint never briefly
    /// re-presents a docked summary as an overlay.
    pub(super) fn sync_right_panel_transition_for_viewport_change(&mut self) -> bool {
        let (width, _) = self.window_state.logical_size();
        self.right_panel_transition
            .sync_targets(targets_for_state(&self.app_state, width), true)
    }

    pub(super) fn advance_right_panel_transition(&mut self, now: Instant) -> TransitionAdvance {
        self.right_panel_transition.advance(now)
    }

    pub(super) fn right_secondary_sidebar_is_present(&self) -> bool {
        self.right_panel_transition.secondary_is_present()
    }

    pub(super) fn right_secondary_sidebar_visibility(&self) -> f32 {
        self.right_panel_transition.secondary_visibility()
    }

    pub(super) fn right_pinned_summary_is_present(&self) -> bool {
        self.right_panel_transition.summary_is_present()
    }

    pub(super) fn right_pinned_summary_visibility(&self) -> f32 {
        self.right_panel_transition.summary_visibility()
    }

    pub(super) fn right_pinned_summary_mode(&self) -> PinnedSummaryMode {
        self.right_panel_transition.summary_mode()
    }
}

#[derive(Debug, Clone)]
struct VisibilityTransition {
    progress: f32,
    target_open: bool,
    last_advanced_at: Option<Instant>,
}

impl Default for VisibilityTransition {
    fn default() -> Self {
        Self {
            progress: 0.0,
            target_open: false,
            last_advanced_at: None,
        }
    }
}

impl VisibilityTransition {
    fn seed(&mut self, open: bool) {
        self.progress = if open { 1.0 } else { 0.0 };
        self.target_open = open;
        self.last_advanced_at = None;
    }

    fn set_target(&mut self, open: bool, reduced_motion: bool) -> bool {
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

    fn advance(&mut self, now: Instant) -> TransitionAdvance {
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

    fn is_present(&self) -> bool {
        self.target_open || self.progress > 0.0
    }

    fn eased_progress(&self) -> f32 {
        sidebar_motion::eased_visibility(self.progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(secondary_open: bool, summary: PinnedSummaryMode) -> RightPanelTargets {
        RightPanelTargets {
            secondary_open,
            summary,
        }
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_1,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn seed_matches_the_persisted_surfaces_without_startup_animation() {
        let transition = RightPanelTransition::seeded(targets(true, PinnedSummaryMode::Overlay));

        assert_near(transition.secondary_visibility(), 1.0);
        assert_near(transition.summary_visibility(), 1.0);
        assert_eq!(transition.summary_mode(), PinnedSummaryMode::Overlay);
    }

    #[test]
    fn closing_keeps_the_outgoing_summary_mode_until_zero() {
        let mut transition =
            RightPanelTransition::seeded(targets(false, PinnedSummaryMode::Overlay));
        assert!(transition.sync_targets(targets(false, PinnedSummaryMode::Hidden), false));
        assert!(transition.summary_is_present());
        assert_eq!(transition.summary_mode(), PinnedSummaryMode::Overlay);

        let start = Instant::now();
        assert!(transition.advance(start).needs_frame);
        let midpoint = transition.advance(start + sidebar_motion::CLOSE_DURATION / 2);
        assert!(midpoint.layout_changed);
        assert!(midpoint.needs_frame);
        assert!(transition.summary_visibility() > 0.0);
        assert_eq!(transition.summary_mode(), PinnedSummaryMode::Overlay);

        let finished = transition.advance(start + sidebar_motion::CLOSE_DURATION);
        assert!(finished.layout_changed);
        assert!(!finished.needs_frame);
        assert!(!transition.summary_is_present());
        assert_eq!(transition.summary_mode(), PinnedSummaryMode::Hidden);
    }

    #[test]
    fn opening_and_reversing_secondary_sidebar_are_continuous() {
        let mut transition =
            RightPanelTransition::seeded(targets(false, PinnedSummaryMode::Hidden));
        transition.sync_targets(targets(true, PinnedSummaryMode::Hidden), false);
        let start = Instant::now();
        transition.advance(start);
        transition.advance(start + sidebar_motion::OPEN_DURATION / 2);
        let opening_progress = transition.secondary_visibility();
        assert!(opening_progress > 0.0 && opening_progress < 1.0);

        transition.sync_targets(targets(false, PinnedSummaryMode::Hidden), false);
        transition.advance(start + sidebar_motion::OPEN_DURATION / 2);
        transition.advance(
            start + sidebar_motion::OPEN_DURATION / 2 + sidebar_motion::CLOSE_DURATION / 4,
        );
        assert!(transition.secondary_visibility() < opening_progress);
        assert!(transition.secondary_is_present());
    }

    #[test]
    fn reduced_motion_snaps_both_surfaces_to_their_targets() {
        let mut transition = RightPanelTransition::seeded(targets(true, PinnedSummaryMode::Docked));
        transition.sync_targets(targets(false, PinnedSummaryMode::Hidden), true);

        assert_near(transition.secondary_visibility(), 0.0);
        assert_near(transition.summary_visibility(), 0.0);
        assert!(!transition.secondary_is_present());
        assert!(!transition.summary_is_present());
    }
}
