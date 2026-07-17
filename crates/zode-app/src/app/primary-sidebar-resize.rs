use zode_app_model::{reduce_presentation_command, AppCommand, PresentationCommandOutcome};
use zode_app_ui::{
    constrained_primary_sidebar_width, PointerButton, PointerEvent, PointerEventKind,
    WorkspaceLayout, PRIMARY_SIDEBAR_MIN_W,
};

use super::DesktopApp;
use crate::cursor::CursorHint;

const PRIMARY_SIDEBAR_RESIZE_HIT_W: f32 = 8.0;

/// How far below the minimum width a drag has to travel before the sidebar
/// commits to collapsing. Picked as a fixed band under `PRIMARY_SIDEBAR_MIN_W`
/// (220 - 40 = 180px) rather than tying it to the viewport-relative minimum:
/// it gives the user a clearly-past-the-minimum gesture to confirm "collapse",
/// rather than collapsing the instant the drag touches the floor, which would
/// make the last few pixels before the minimum feel like a trap door.
const PRIMARY_SIDEBAR_COLLAPSE_BAND: f32 = 40.0;

impl DesktopApp {
    pub(super) fn handle_primary_sidebar_resize_pointer(&mut self, event: PointerEvent) -> bool {
        // Only refuse to *start* a resize while some other transition (e.g. a
        // keyboard toggle) is mid-flight. An already-active drag may itself
        // be the thing driving the transition (auto-collapse/restore below)
        // and must keep tracking the pointer through it - the geometry both
        // paths read (`sidebar.origin.x` and the sidebar+surface+divider+
        // review width sum) stays invariant across the animation, so nothing
        // here depends on the transition being settled.
        if !self.window_state.primary_sidebar_resize_active
            && self.primary_sidebar_transition_is_active()
        {
            self.set_primary_sidebar_resize_cursor(false);
            return false;
        }
        let intent = primary_sidebar_resize_intent(
            self.window_state.primary_sidebar_resize_active,
            self.app_state.shell.sidebar_open,
            event,
            self.frame_snapshot.layout,
        );
        match intent {
            PrimarySidebarResizeIntent::Ignored => false,
            PrimarySidebarResizeIntent::Hover => {
                self.set_primary_sidebar_resize_cursor(true);
                true
            }
            PrimarySidebarResizeIntent::Begin => {
                self.window_state.primary_sidebar_resize_active = true;
                self.set_primary_sidebar_resize_cursor(true);
                true
            }
            PrimarySidebarResizeIntent::Resize(width) => {
                if reduce_presentation_command(
                    &mut self.app_state,
                    AppCommand::SetPrimarySidebarWidth(width),
                ) == PresentationCommandOutcome::Applied
                {
                    self.rebuild_frame_snapshot();
                    self.request_redraw();
                }
                self.set_primary_sidebar_resize_cursor(true);
                true
            }
            PrimarySidebarResizeIntent::Collapse => {
                // Same path the toggle button uses, so the sidebar animates
                // closed instead of snapping. `primary_sidebar_width` is left
                // untouched, which is what lets the eventual re-expand
                // restore the pre-drag width instead of a collapsed one.
                self.arm_primary_sidebar_preview_suppression();
                reduce_presentation_command(&mut self.app_state, AppCommand::TogglePrimarySidebar);
                self.sync_primary_sidebar_transition();
                self.rebuild_frame_snapshot();
                self.request_redraw();
                self.set_primary_sidebar_resize_cursor(true);
                true
            }
            PrimarySidebarResizeIntent::Restore(width) => {
                // Hysteresis: dragging back above the threshold within the
                // same gesture reopens instead of requiring a release.
                reduce_presentation_command(&mut self.app_state, AppCommand::TogglePrimarySidebar);
                reduce_presentation_command(
                    &mut self.app_state,
                    AppCommand::SetPrimarySidebarWidth(width),
                );
                self.sync_primary_sidebar_transition();
                self.rebuild_frame_snapshot();
                self.request_redraw();
                self.set_primary_sidebar_resize_cursor(true);
                true
            }
            PrimarySidebarResizeIntent::Finish => {
                self.window_state.primary_sidebar_resize_active = false;
                self.persist_ui_state();
                let still_over_handle = primary_sidebar_resize_handle(self.frame_snapshot.layout)
                    .is_some_and(|rect| rect.contains(event.position));
                self.set_primary_sidebar_resize_cursor(still_over_handle);
                true
            }
        }
    }

    fn set_primary_sidebar_resize_cursor(&mut self, resize: bool) {
        self.update_native_cursor(if resize {
            CursorHint::ResizeEw
        } else {
            CursorHint::Default
        });
    }

    pub(super) fn cancel_primary_sidebar_resize(&mut self) {
        self.ensure_frame_snapshot();
        if finish_primary_sidebar_resize(&mut self.window_state.primary_sidebar_resize_active) {
            self.persist_ui_state();
        }
        self.set_primary_sidebar_resize_cursor(false);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimarySidebarResizeIntent {
    Ignored,
    Hover,
    Begin,
    Resize(u16),
    /// The drag crossed below the collapse threshold while the sidebar was
    /// open.
    Collapse,
    /// The drag crossed back above the collapse threshold, mid-gesture,
    /// while the sidebar was collapsed; carries the width to resume at.
    Restore(u16),
    Finish,
}

fn primary_sidebar_resize_intent(
    active: bool,
    sidebar_open: bool,
    event: PointerEvent,
    layout: WorkspaceLayout,
) -> PrimarySidebarResizeIntent {
    if active {
        return match (event.kind, event.button) {
            (PointerEventKind::Move, _) => {
                let requested = requested_primary_sidebar_width(layout, event.position.x);
                if requested < primary_sidebar_collapse_threshold() {
                    if sidebar_open {
                        PrimarySidebarResizeIntent::Collapse
                    } else {
                        PrimarySidebarResizeIntent::Ignored
                    }
                } else if sidebar_open {
                    PrimarySidebarResizeIntent::Resize(clamp_primary_sidebar_width(
                        layout, requested,
                    ))
                } else {
                    PrimarySidebarResizeIntent::Restore(clamp_primary_sidebar_width(
                        layout, requested,
                    ))
                }
            }
            (PointerEventKind::Release, Some(PointerButton::Primary)) => {
                PrimarySidebarResizeIntent::Finish
            }
            _ => PrimarySidebarResizeIntent::Ignored,
        };
    }
    let over_handle =
        primary_sidebar_resize_handle(layout).is_some_and(|handle| handle.contains(event.position));
    match (event.kind, event.button, over_handle) {
        (PointerEventKind::Move, _, true) => PrimarySidebarResizeIntent::Hover,
        (PointerEventKind::Press, Some(PointerButton::Primary), true) => {
            PrimarySidebarResizeIntent::Begin
        }
        _ => PrimarySidebarResizeIntent::Ignored,
    }
}

fn primary_sidebar_resize_handle(layout: WorkspaceLayout) -> Option<jian_widgets::Rect> {
    (layout.sidebar.size.x > 0.0 && layout.primary_sidebar_divider.size.y > 0.0).then(|| {
        jian_widgets::Rect::xywh(
            layout.primary_sidebar_divider.origin.x - PRIMARY_SIDEBAR_RESIZE_HIT_W / 2.0,
            layout.primary_sidebar_divider.origin.y,
            PRIMARY_SIDEBAR_RESIZE_HIT_W,
            layout.primary_sidebar_divider.size.y,
        )
    })
}

fn primary_sidebar_collapse_threshold() -> f32 {
    PRIMARY_SIDEBAR_MIN_W - PRIMARY_SIDEBAR_COLLAPSE_BAND
}

/// Raw pointer-relative width request, unclamped. `sidebar.origin.x` and the
/// sidebar+primary_surface+divider+review_panel width sum are both invariant
/// under the primary sidebar's animated visibility (shrinking sidebar width
/// is exactly offset by a growing primary surface), so this stays a stable
/// read whether or not a collapse/restore transition is mid-flight.
fn requested_primary_sidebar_width(layout: WorkspaceLayout, pointer_x: f32) -> f32 {
    pointer_x - layout.sidebar.origin.x
}

fn clamp_primary_sidebar_width(layout: WorkspaceLayout, requested: f32) -> u16 {
    let available_width = layout.sidebar.size.x
        + layout.primary_surface.size.x
        + layout.divider.size.x
        + layout.review_panel.size.x;
    let width = constrained_primary_sidebar_width(available_width, requested).round();
    width.clamp(0.0, f32::from(u16::MAX)) as u16
}

fn finish_primary_sidebar_resize(active: &mut bool) -> bool {
    std::mem::replace(active, false)
}

#[cfg(test)]
mod tests {
    use jian_widgets::Point2D;
    use zode_app_model::{demo_state, reduce_presentation_command, AppCommand};
    use zode_app_ui::{
        Insets, PointerButton, PointerEvent, PointerEventKind, PrimarySidebarLayoutOptions,
        WorkspaceLayout, WorkspaceLayoutOptions,
    };

    use super::{
        finish_primary_sidebar_resize, primary_sidebar_resize_handle,
        primary_sidebar_resize_intent, PrimarySidebarResizeIntent,
    };

    fn open_layout() -> WorkspaceLayout {
        WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions {
                primary_sidebar: PrimarySidebarLayoutOptions {
                    open: true,
                    width: 293.0,
                    visibility: 1.0,
                },
                ..WorkspaceLayoutOptions::default()
            },
        )
    }

    fn move_at(x: f32) -> PointerEvent {
        PointerEvent {
            position: Point2D::new(x, 400.0),
            kind: PointerEventKind::Move,
            button: None,
        }
    }

    #[test]
    fn drag_intent_tracks_the_primary_split_handle() {
        let layout = open_layout();
        let move_event = move_at(293.0);
        let press_event = PointerEvent {
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            ..move_event
        };

        assert!(primary_sidebar_resize_handle(layout).is_some());
        assert_eq!(
            primary_sidebar_resize_intent(false, true, move_event, layout),
            PrimarySidebarResizeIntent::Hover
        );
        assert_eq!(
            primary_sidebar_resize_intent(false, true, press_event, layout),
            PrimarySidebarResizeIntent::Begin
        );
        assert_eq!(
            primary_sidebar_resize_intent(true, true, move_at(340.0), layout),
            PrimarySidebarResizeIntent::Resize(340)
        );
        assert_eq!(
            primary_sidebar_resize_intent(
                true,
                true,
                PointerEvent {
                    position: Point2D::new(340.0, 400.0),
                    kind: PointerEventKind::Release,
                    button: Some(PointerButton::Primary),
                },
                layout,
            ),
            PrimarySidebarResizeIntent::Finish
        );
    }

    #[test]
    fn drag_persists_the_same_width_that_the_viewport_can_render() {
        let initial = WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions::default(),
        );
        let PrimarySidebarResizeIntent::Resize(width) =
            primary_sidebar_resize_intent(true, true, move_at(900.0), initial)
        else {
            panic!("active drag did not produce a resize intent");
        };

        assert_eq!(width, 540);
        let mut state = demo_state();
        let _ = reduce_presentation_command(&mut state, AppCommand::SetPrimarySidebarWidth(width));
        let visible = WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions {
                primary_sidebar: PrimarySidebarLayoutOptions {
                    open: true,
                    width: f32::from(state.ui_preferences.primary_sidebar_width),
                    visibility: 1.0,
                },
                ..WorkspaceLayoutOptions::default()
            },
        );

        assert_eq!(state.ui_preferences.primary_sidebar_width, 540);
        assert_eq!(visible.sidebar.size.x, 540.0);
    }

    #[test]
    fn cursor_exit_or_focus_loss_clears_the_active_drag_latch() {
        let mut active = true;
        assert!(finish_primary_sidebar_resize(&mut active));
        assert!(!active);
        assert!(!finish_primary_sidebar_resize(&mut active));
    }

    #[test]
    fn hidden_primary_sidebar_has_no_drag_handle() {
        let layout = WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions {
                primary_sidebar: PrimarySidebarLayoutOptions {
                    open: false,
                    width: 293.0,
                    visibility: 1.0,
                },
                ..WorkspaceLayoutOptions::default()
            },
        );
        let event = PointerEvent {
            position: Point2D::new(0.0, 400.0),
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
        };

        assert!(primary_sidebar_resize_handle(layout).is_none());
        assert_eq!(
            primary_sidebar_resize_intent(false, false, event, layout),
            PrimarySidebarResizeIntent::Ignored
        );
    }

    #[test]
    fn dragging_past_the_threshold_collapses_instead_of_clamping_at_the_minimum() {
        let layout = open_layout();
        // sidebar.origin.x is 0 here, so raw requested width equals pointer x.
        // Threshold is MIN_W(220) - 40 = 180; 150 is well past it.
        assert_eq!(
            primary_sidebar_resize_intent(true, true, move_at(150.0), layout),
            PrimarySidebarResizeIntent::Collapse
        );
        // Just above the threshold still resizes (clamped to the minimum),
        // it does not collapse.
        assert_eq!(
            primary_sidebar_resize_intent(true, true, move_at(200.0), layout),
            PrimarySidebarResizeIntent::Resize(220)
        );
    }

    #[test]
    fn once_collapsed_further_sub_threshold_moves_are_a_no_op() {
        let layout = open_layout();
        assert_eq!(
            primary_sidebar_resize_intent(true, false, move_at(50.0), layout),
            PrimarySidebarResizeIntent::Ignored
        );
    }

    #[test]
    fn dragging_back_above_the_threshold_restores_with_hysteresis() {
        let layout = open_layout();
        // Collapsed mid-gesture, then dragged back to 200 (above the 180
        // threshold but below the 220 minimum): restores, clamped to the
        // minimum width rather than flapping open at a sub-minimum size.
        assert_eq!(
            primary_sidebar_resize_intent(true, false, move_at(200.0), layout),
            PrimarySidebarResizeIntent::Restore(220)
        );
        // Comfortably above the minimum restores at the requested width.
        assert_eq!(
            primary_sidebar_resize_intent(true, false, move_at(300.0), layout),
            PrimarySidebarResizeIntent::Restore(300)
        );
    }

    #[test]
    fn releasing_below_the_threshold_ends_collapsed_and_remembers_the_pre_drag_width() {
        let mut state = demo_state();
        assert_eq!(state.ui_preferences.primary_sidebar_width, 293);
        assert!(state.shell.sidebar_open);

        // The drag collapses (handled by the host as a TogglePrimarySidebar,
        // exercised here directly since this test stays at the reducer
        // level); width is deliberately left untouched by a Collapse intent.
        let _ = reduce_presentation_command(&mut state, AppCommand::TogglePrimarySidebar);
        assert!(!state.shell.sidebar_open);
        assert_eq!(
            state.ui_preferences.primary_sidebar_width, 293,
            "collapsing must never rewrite the remembered pre-drag width"
        );

        // Release below the threshold: no further width command is issued,
        // so the stored width is still the pre-drag value and is never a
        // sub-minimum value.
        assert!(state.ui_preferences.primary_sidebar_width >= 220);
    }

    #[test]
    fn re_expanding_after_a_drag_collapse_restores_the_pre_drag_width() {
        let mut state = demo_state();
        let _ = reduce_presentation_command(&mut state, AppCommand::SetPrimarySidebarWidth(420));
        let _ = reduce_presentation_command(&mut state, AppCommand::TogglePrimarySidebar);
        assert!(!state.shell.sidebar_open);

        let _ = reduce_presentation_command(&mut state, AppCommand::TogglePrimarySidebar);
        assert!(state.shell.sidebar_open);
        assert_eq!(state.ui_preferences.primary_sidebar_width, 420);
    }
}
