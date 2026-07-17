use std::time::{Duration, Instant};

use jian_widgets::{Point2D, Rect};

pub(super) const OPEN_DURATION: Duration = Duration::from_millis(160);
pub(super) const CLOSE_DURATION: Duration = Duration::from_millis(120);

/// Stable pointer regions for the transient primary-sidebar preview.
///
/// The overlay rect must describe the sidebar's final, fully-open geometry,
/// not its animated paint position. Keeping hit geometry fixed prevents the
/// preview from closing as soon as the sidebar starts sliding away from the
/// trigger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PrimarySidebarPreviewRegions {
    pub trigger: Rect,
    pub overlay: Rect,
    /// Optional bridge from the sidebar edge through an open sidebar menu.
    /// Callers should include both the visual gap and the menu surface.
    pub menu_corridor: Option<Rect>,
}

impl PrimarySidebarPreviewRegions {
    pub const fn new(trigger: Rect, overlay: Rect, menu_corridor: Option<Rect>) -> Self {
        Self {
            trigger,
            overlay,
            menu_corridor,
        }
    }

    fn trigger_contains(self, point: Point2D) -> bool {
        self.trigger.contains(point)
    }

    fn keepalive_contains(self, point: Point2D) -> bool {
        self.overlay.contains(point)
            || self
                .menu_corridor
                .is_some_and(|corridor| corridor.contains(point))
    }
}

/// Conditions that suppress the transient preview.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PrimarySidebarPreviewContext {
    pub persistent_open: bool,
    pub settings_active: bool,
    pub blocking_overlay_open: bool,
    pub reduced_motion: bool,
}

impl PrimarySidebarPreviewContext {
    fn blocked(self) -> bool {
        self.persistent_open || self.settings_active || self.blocking_overlay_open
    }
}

/// Host-owned animation state for the hover-only primary-sidebar preview.
///
/// Persistent sidebar visibility remains in `ZodeAppState`; this state is
/// deliberately transient so hovering never mutates or persists shell layout.
#[derive(Debug, Clone)]
pub(super) struct PrimarySidebarPreview {
    progress: f32,
    target_open: bool,
    last_advanced_at: Option<Instant>,
    /// Armed right when an explicit collapse (toggle button, shortcut, or
    /// drag-to-collapse) fires while the pointer sits inside the trigger
    /// zone. A collapse never moves the button, so the pointer is still
    /// inside the zone on the very next hover check; without this latch the
    /// preview would slide back in immediately after the sidebar closes.
    /// Cleared the first time a hover check finds the pointer outside the
    /// trigger zone (or absent), so leaving and re-entering still opens it.
    suppress_until_pointer_exit: bool,
}

impl Default for PrimarySidebarPreview {
    fn default() -> Self {
        Self {
            progress: 0.0,
            target_open: false,
            last_advanced_at: None,
            suppress_until_pointer_exit: false,
        }
    }
}

impl PrimarySidebarPreview {
    pub fn progress(&self) -> f32 {
        self.progress
    }

    #[cfg(test)]
    pub fn target(&self) -> bool {
        self.target_open
    }

    /// The overlay remains present while closing so pointer re-entry can
    /// reverse the transition without flashing.
    pub fn is_visible(&self) -> bool {
        self.target_open || self.progress > 0.0
    }

    pub fn dismiss(&mut self) -> bool {
        self.reset()
    }

    /// Arms the post-collapse suppression latch. Call this at the point an
    /// explicit collapse is decided, before the sidebar geometry changes.
    pub fn suppress_until_pointer_exit(&mut self) {
        self.suppress_until_pointer_exit = true;
    }

    #[cfg(test)]
    pub fn is_suppressed(&self) -> bool {
        self.suppress_until_pointer_exit
    }

    pub fn transition_duration(&self) -> Duration {
        if self.target_open {
            OPEN_DURATION
        } else {
            CLOSE_DURATION
        }
    }

    /// Re-evaluate hover intent from stable geometry.
    ///
    /// A hidden preview can only be opened by the trigger. Once visible, the
    /// final overlay rect and optional menu corridor keep it open. Suppression
    /// conditions clear it immediately to avoid duplicate or obstructing UI.
    pub fn update_hover(
        &mut self,
        pointer: Option<Point2D>,
        regions: PrimarySidebarPreviewRegions,
        context: PrimarySidebarPreviewContext,
    ) -> bool {
        if context.blocked() {
            return self.reset();
        }

        if self.suppress_until_pointer_exit {
            let still_inside_trigger = pointer.is_some_and(|point| regions.trigger_contains(point));
            if still_inside_trigger {
                return self.set_target(false, context.reduced_motion);
            }
            self.suppress_until_pointer_exit = false;
        }

        let wants_open = pointer.is_some_and(|point| {
            regions.trigger_contains(point)
                || (self.is_visible() && regions.keepalive_contains(point))
        });
        self.set_target(wants_open, context.reduced_motion)
    }

    /// Advance the current transition and return whether another animation
    /// frame is required. Call once immediately after a target change to seed
    /// the clock, then on each redraw while this method returns `true`.
    pub fn advance(&mut self, now: Instant, duration: Duration) -> bool {
        let target_progress = if self.target_open { 1.0 } else { 0.0 };
        if self.progress == target_progress {
            self.last_advanced_at = None;
            return false;
        }
        if duration.is_zero() {
            self.progress = target_progress;
            self.last_advanced_at = None;
            return false;
        }

        let Some(previous) = self.last_advanced_at.replace(now) else {
            return true;
        };
        let elapsed = now.checked_duration_since(previous).unwrap_or_default();
        let delta = elapsed.as_secs_f32() / duration.as_secs_f32();
        if self.target_open {
            self.progress = (self.progress + delta).min(1.0);
        } else {
            self.progress = (self.progress - delta).max(0.0);
        }

        let needs_frame = self.progress != target_progress;
        if !needs_frame {
            self.last_advanced_at = None;
        }
        needs_frame
    }

    fn set_target(&mut self, target_open: bool, reduced_motion: bool) -> bool {
        let previous_target = self.target_open;
        let previous_progress = self.progress;
        if self.target_open != target_open {
            self.target_open = target_open;
            self.last_advanced_at = None;
        }
        if reduced_motion {
            self.progress = if target_open { 1.0 } else { 0.0 };
            self.last_advanced_at = None;
        }
        previous_target != self.target_open || previous_progress != self.progress
    }

    fn reset(&mut self) -> bool {
        let changed = self.target_open || self.progress > 0.0 || self.last_advanced_at.is_some();
        self.target_open = false;
        self.progress = 0.0;
        self.last_advanced_at = None;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regions() -> PrimarySidebarPreviewRegions {
        PrimarySidebarPreviewRegions::new(
            Rect::xywh(88.0, 11.0, 24.0, 24.0),
            Rect::xywh(0.0, 0.0, 240.0, 900.0),
            Some(Rect::xywh(240.0, 100.0, 228.0, 320.0)),
        )
    }

    fn point(x: f32, y: f32) -> Point2D {
        Point2D::new(x, y)
    }

    fn assert_progress(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_1,
            "expected progress {expected}, got {actual}"
        );
    }

    #[test]
    fn starts_hidden() {
        let preview = PrimarySidebarPreview::default();

        assert!(!preview.target());
        assert!(!preview.is_visible());
        assert_progress(preview.progress(), 0.0);
        assert_eq!(preview.transition_duration(), CLOSE_DURATION);
    }

    #[test]
    fn trigger_opens_and_advances_over_open_duration() {
        let mut preview = PrimarySidebarPreview::default();
        assert!(preview.update_hover(
            Some(point(96.0, 20.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        ));
        assert!(preview.target());
        assert!(preview.is_visible());
        assert_eq!(preview.transition_duration(), OPEN_DURATION);

        let start = Instant::now();
        assert!(preview.advance(start, OPEN_DURATION));
        assert!(preview.advance(start + Duration::from_millis(80), OPEN_DURATION));
        assert_progress(preview.progress(), 0.5);
        assert!(!preview.advance(start + OPEN_DURATION, OPEN_DURATION));
        assert_progress(preview.progress(), 1.0);
    }

    #[test]
    fn overlay_and_menu_corridor_only_keep_an_existing_preview_open() {
        let mut preview = PrimarySidebarPreview::default();
        let context = PrimarySidebarPreviewContext::default();

        assert!(!preview.update_hover(Some(point(180.0, 500.0)), regions(), context));
        assert!(!preview.target());

        assert!(preview.update_hover(Some(point(96.0, 20.0)), regions(), context));
        assert!(!preview.update_hover(Some(point(180.0, 500.0)), regions(), context));
        assert!(preview.target());
        assert!(!preview.update_hover(Some(point(246.0, 130.0)), regions(), context));
        assert!(preview.target());
    }

    #[test]
    fn leaving_all_regions_closes_over_close_duration() {
        let mut preview = PrimarySidebarPreview::default();
        let context = PrimarySidebarPreviewContext {
            reduced_motion: true,
            ..PrimarySidebarPreviewContext::default()
        };
        preview.update_hover(Some(point(96.0, 20.0)), regions(), context);
        assert_progress(preview.progress(), 1.0);

        assert!(preview.update_hover(
            Some(point(700.0, 500.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        ));
        assert!(!preview.target());
        assert!(preview.is_visible());
        assert_eq!(preview.transition_duration(), CLOSE_DURATION);

        let start = Instant::now();
        assert!(preview.advance(start, CLOSE_DURATION));
        assert!(preview.advance(start + Duration::from_millis(60), CLOSE_DURATION));
        assert_progress(preview.progress(), 0.5);
        assert!(!preview.advance(start + CLOSE_DURATION, CLOSE_DURATION));
        assert!(!preview.is_visible());
    }

    #[test]
    fn persistent_settings_and_blocking_overlays_clear_immediately() {
        for context in [
            PrimarySidebarPreviewContext {
                persistent_open: true,
                ..PrimarySidebarPreviewContext::default()
            },
            PrimarySidebarPreviewContext {
                settings_active: true,
                ..PrimarySidebarPreviewContext::default()
            },
            PrimarySidebarPreviewContext {
                blocking_overlay_open: true,
                ..PrimarySidebarPreviewContext::default()
            },
        ] {
            let mut preview = PrimarySidebarPreview::default();
            preview.update_hover(
                Some(point(96.0, 20.0)),
                regions(),
                PrimarySidebarPreviewContext {
                    reduced_motion: true,
                    ..PrimarySidebarPreviewContext::default()
                },
            );

            assert!(preview.update_hover(Some(point(96.0, 20.0)), regions(), context));
            assert!(!preview.target());
            assert!(!preview.is_visible());
            assert_progress(preview.progress(), 0.0);
        }
    }

    #[test]
    fn reduced_motion_snaps_both_directions() {
        let mut preview = PrimarySidebarPreview::default();
        let context = PrimarySidebarPreviewContext {
            reduced_motion: true,
            ..PrimarySidebarPreviewContext::default()
        };

        assert!(preview.update_hover(Some(point(96.0, 20.0)), regions(), context));
        assert_progress(preview.progress(), 1.0);
        assert!(!preview.advance(Instant::now(), OPEN_DURATION));

        assert!(preview.update_hover(None, regions(), context));
        assert_progress(preview.progress(), 0.0);
        assert!(!preview.is_visible());
    }

    #[test]
    fn reentering_during_close_reverses_without_hiding() {
        let mut preview = PrimarySidebarPreview::default();
        preview.update_hover(
            Some(point(96.0, 20.0)),
            regions(),
            PrimarySidebarPreviewContext {
                reduced_motion: true,
                ..PrimarySidebarPreviewContext::default()
            },
        );
        preview.update_hover(
            Some(point(700.0, 500.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        );
        let start = Instant::now();
        preview.advance(start, CLOSE_DURATION);
        preview.advance(start + Duration::from_millis(30), CLOSE_DURATION);
        assert_progress(preview.progress(), 0.75);

        assert!(preview.update_hover(
            Some(point(180.0, 500.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        ));
        assert!(preview.target());
        assert!(preview.is_visible());
        assert_progress(preview.progress(), 0.75);
    }

    #[test]
    fn suppression_blocks_the_preview_while_the_pointer_stays_in_the_trigger_zone() {
        let mut preview = PrimarySidebarPreview::default();
        preview.suppress_until_pointer_exit();
        assert!(preview.is_suppressed());

        // The pointer never moved off the button the collapse just fired
        // from, so repeated hover checks at the same point must stay closed.
        for _ in 0..3 {
            assert!(!preview.update_hover(
                Some(point(96.0, 20.0)),
                regions(),
                PrimarySidebarPreviewContext::default(),
            ));
            assert!(!preview.target());
            assert!(preview.is_suppressed());
        }
    }

    #[test]
    fn suppression_clears_once_the_pointer_leaves_the_trigger_zone_and_reentry_opens() {
        let mut preview = PrimarySidebarPreview::default();
        preview.suppress_until_pointer_exit();

        // Leaving the zone clears the latch without opening the preview.
        assert!(!preview.update_hover(
            Some(point(700.0, 500.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        ));
        assert!(!preview.is_suppressed());
        assert!(!preview.target());

        // A fresh entry after that is a normal hover and opens the preview.
        assert!(preview.update_hover(
            Some(point(96.0, 20.0)),
            regions(),
            PrimarySidebarPreviewContext::default(),
        ));
        assert!(preview.target());
    }

    #[test]
    fn suppression_clears_when_the_pointer_leaves_the_window_entirely() {
        let mut preview = PrimarySidebarPreview::default();
        preview.suppress_until_pointer_exit();

        assert!(!preview.update_hover(None, regions(), PrimarySidebarPreviewContext::default()));
        assert!(!preview.is_suppressed());
    }

    #[test]
    fn a_blocking_context_still_forces_the_preview_closed_while_suppressed() {
        let mut preview = PrimarySidebarPreview::default();
        preview.suppress_until_pointer_exit();

        assert!(!preview.update_hover(
            Some(point(96.0, 20.0)),
            regions(),
            PrimarySidebarPreviewContext {
                persistent_open: true,
                ..PrimarySidebarPreviewContext::default()
            },
        ));
        assert!(!preview.target());
    }
}
