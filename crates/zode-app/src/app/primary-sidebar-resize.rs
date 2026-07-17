use zode_app_model::{reduce_presentation_command, AppCommand, PresentationCommandOutcome};
use zode_app_ui::{
    constrained_primary_sidebar_width, PointerButton, PointerEvent, PointerEventKind,
    WorkspaceLayout,
};

use super::DesktopApp;
use crate::cursor::CursorHint;

const PRIMARY_SIDEBAR_RESIZE_HIT_W: f32 = 8.0;

impl DesktopApp {
    pub(super) fn handle_primary_sidebar_resize_pointer(&mut self, event: PointerEvent) -> bool {
        let intent = primary_sidebar_resize_intent(
            self.window_state.primary_sidebar_resize_active,
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
    Finish,
}

fn primary_sidebar_resize_intent(
    active: bool,
    event: PointerEvent,
    layout: WorkspaceLayout,
) -> PrimarySidebarResizeIntent {
    if active {
        return match (event.kind, event.button) {
            (PointerEventKind::Move, _) => PrimarySidebarResizeIntent::Resize(
                primary_sidebar_width_at_pointer(layout, event.position.x),
            ),
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

fn primary_sidebar_width_at_pointer(layout: WorkspaceLayout, pointer_x: f32) -> u16 {
    let requested = pointer_x - layout.sidebar.origin.x;
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

    #[test]
    fn drag_intent_tracks_the_primary_split_handle() {
        let layout = WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions {
                primary_sidebar: PrimarySidebarLayoutOptions {
                    open: true,
                    width: 293.0,
                },
                ..WorkspaceLayoutOptions::default()
            },
        );
        let move_event = PointerEvent {
            position: Point2D::new(293.0, 400.0),
            kind: PointerEventKind::Move,
            button: None,
        };
        let press_event = PointerEvent {
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            ..move_event
        };

        assert!(primary_sidebar_resize_handle(layout).is_some());
        assert_eq!(
            primary_sidebar_resize_intent(false, move_event, layout),
            PrimarySidebarResizeIntent::Hover
        );
        assert_eq!(
            primary_sidebar_resize_intent(false, press_event, layout),
            PrimarySidebarResizeIntent::Begin
        );
        assert_eq!(
            primary_sidebar_resize_intent(
                true,
                PointerEvent {
                    position: Point2D::new(340.0, 400.0),
                    kind: PointerEventKind::Move,
                    button: None,
                },
                layout,
            ),
            PrimarySidebarResizeIntent::Resize(340)
        );
        assert_eq!(
            primary_sidebar_resize_intent(
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
        let event = PointerEvent {
            position: Point2D::new(900.0, 400.0),
            kind: PointerEventKind::Move,
            button: None,
        };
        let PrimarySidebarResizeIntent::Resize(width) =
            primary_sidebar_resize_intent(true, event, initial)
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
            primary_sidebar_resize_intent(false, event, layout),
            PrimarySidebarResizeIntent::Ignored
        );
    }
}
