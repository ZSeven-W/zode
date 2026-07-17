pub use jian_core::CursorHint;
use jian_widgets::Point2D;
use winit::window::CursorIcon;
use zode_app_ui::{WidgetId, WorkspaceSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CachedCursor {
    hint: CursorHint,
    icon: CursorIcon,
}

/// Tracks the cursor last submitted to the native window so high-frequency
/// pointer motion does not repeat the same platform call.
#[derive(Debug, Default)]
pub(crate) struct NativeCursorState {
    current: Option<CachedCursor>,
}

impl NativeCursorState {
    pub(crate) fn changed_icon(&mut self, hint: CursorHint) -> Option<CursorIcon> {
        let next = CachedCursor {
            hint,
            icon: cursor_icon_for_hint(hint),
        };
        if self.current == Some(next) {
            return None;
        }
        self.current = Some(next);
        Some(next.icon)
    }

    pub(crate) fn invalidate(&mut self) {
        self.current = None;
    }
}

pub fn cursor_icon_for_hint(hint: CursorHint) -> CursorIcon {
    match hint {
        CursorHint::Default => CursorIcon::Default,
        CursorHint::Pointer => CursorIcon::Pointer,
        CursorHint::Move => CursorIcon::Move,
        CursorHint::Grabbing => CursorIcon::Grabbing,
        CursorHint::Grab => CursorIcon::Grab,
        CursorHint::Crosshair => CursorIcon::Crosshair,
        CursorHint::Text => CursorIcon::Text,
        CursorHint::NotAllowed => CursorIcon::NotAllowed,
        CursorHint::ResizeEw => CursorIcon::EwResize,
        CursorHint::ResizeNs => CursorIcon::NsResize,
        CursorHint::ResizeNwse => CursorIcon::NwseResize,
        CursorHint::ResizeNesw => CursorIcon::NeswResize,
        CursorHint::Rotate => CursorIcon::Alias,
    }
}

pub fn cursor_hint_at(snapshot: &WorkspaceSnapshot, point: Point2D) -> CursorHint {
    cursor_hint_for_hit(snapshot, snapshot.hit_test(point))
}

pub(crate) fn cursor_hint_for_hit(
    snapshot: &WorkspaceSnapshot,
    hit: Option<WidgetId>,
) -> CursorHint {
    hit.and_then(|id| snapshot.node(id))
        .map_or(CursorHint::Default, |node| node.cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cursor_state_only_emits_semantic_changes() {
        let mut state = NativeCursorState::default();

        assert_eq!(state.changed_icon(CursorHint::Text), Some(CursorIcon::Text));
        assert_eq!(state.changed_icon(CursorHint::Text), None);
        assert_eq!(
            state.changed_icon(CursorHint::ResizeEw),
            Some(CursorIcon::EwResize)
        );
        assert_eq!(state.changed_icon(CursorHint::ResizeEw), None);
        assert_eq!(
            state.changed_icon(CursorHint::Default),
            Some(CursorIcon::Default)
        );
        assert_eq!(state.changed_icon(CursorHint::Default), None);
    }

    #[test]
    fn invalidating_native_cursor_forces_the_next_submission() {
        let mut state = NativeCursorState::default();

        assert_eq!(
            state.changed_icon(CursorHint::Pointer),
            Some(CursorIcon::Pointer)
        );
        state.invalidate();
        assert_eq!(
            state.changed_icon(CursorHint::Pointer),
            Some(CursorIcon::Pointer)
        );
    }
}
