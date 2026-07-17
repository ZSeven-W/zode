use accesskit::{Action, Role};
use jian_core::CursorHint;
use jian_widgets::Rect;
use zode_app_model::ZodeAppState;

use super::{next_order, node, visible_rect, InteractionNode};
use crate::{Composer, EmptyState, RectExt, WorkspaceLayout, TRANSCRIPT_COMPOSER_GAP};

pub(super) fn append_empty_suggestion_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    if state.current_session.is_some()
        || state.project_picker.open
        || !state.ui_preferences.task_suggestions
    {
        return;
    }
    let input = Composer::layout_for_state(layout.composer, state).input;
    let empty_bottom = (input.origin.y - TRANSCRIPT_COMPOSER_GAP)
        .max(layout.transcript.origin.y)
        .min(layout.primary_surface.max_y());
    let empty = Rect::xywh(
        layout.transcript.origin.x,
        layout.transcript.origin.y,
        layout.transcript.size.x,
        empty_bottom - layout.transcript.origin.y,
    );
    for suggestion in EmptyState::suggestion_layouts(empty) {
        if !visible_rect(suggestion.rect) {
            continue;
        }
        nodes.push(node(
            suggestion.id,
            suggestion.rect,
            Role::Button,
            suggestion.label,
            Some("填入任务输入框".into()),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}
