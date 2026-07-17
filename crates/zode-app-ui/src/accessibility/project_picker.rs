use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use jian_widgets::Rect;
use zode_app_model::{ShellRoute, ZodeAppState};

use super::{next_order, node, visible_rect, InteractionNode};
use crate::{
    Composer, EmptyState, ProjectPicker, ProjectPickerViewState, RectExt, WorkspaceLayout,
    PROJECT_DETACH_ID, PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_TRIGGER_ID,
    TRANSCRIPT_COMPOSER_GAP,
};

pub(super) fn append_welcome_project_trigger(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some((label, trigger)) = welcome_project_trigger(layout, state) else {
        return;
    };
    nodes.push(node(
        PROJECT_PICKER_TRIGGER_ID,
        expand_and_clip(trigger, layout.primary_surface, 6.0, 4.0),
        Role::Button,
        "切换项目",
        Some(label),
        vec![Action::Click, Action::Focus],
        next_order(focus_order),
        CursorHint::Pointer,
    ));
}

pub(super) fn append_composer_detach(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    if state.current_session.is_some() {
        return;
    }
    let Some(label) = ProjectPicker::active_workspace_label(state) else {
        return;
    };
    let context = Composer::context_layout(layout.composer, state, Some(&label));
    let Some(detach) = context.detach.filter(|rect| visible_rect(*rect)) else {
        return;
    };
    nodes.push(node(
        PROJECT_DETACH_ID,
        detach,
        Role::Button,
        "不在项目中工作",
        None,
        vec![Action::Click, Action::Focus],
        next_order(focus_order),
        CursorHint::Pointer,
    ));
}

pub(super) fn append_picker_overlay(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
    picker: &ProjectPickerViewState,
) -> Option<crate::WidgetId> {
    let (_, trigger) = welcome_project_trigger(layout, state)?;
    let picker_layout = ProjectPicker::layout(layout.viewport, trigger, state, picker)?;
    nodes.push(node(
        crate::PROJECT_PICKER_SURFACE_ID,
        picker_layout.surface,
        Role::Menu,
        "项目选择器",
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    nodes.push(node(
        PROJECT_PICKER_SEARCH_ID,
        picker_layout.search,
        Role::SearchInput,
        "搜索项目",
        Some(picker.query.clone()),
        vec![Action::Focus, Action::SetValue],
        next_order(focus_order),
        CursorHint::Text,
    ));
    for row in picker_layout.rows {
        let mut row_node = node(
            row.id,
            row.rect,
            Role::MenuItem,
            &format!("切换到项目 {}", row.label),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        row_node.toggled = Some(Toggled::from(row.selected));
        nodes.push(row_node);
    }
    for row in [picker_layout.new_project, picker_layout.projectless] {
        let mut row_node = node(
            row.id,
            row.rect,
            Role::MenuItem,
            &row.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        row_node.toggled = row.selected.then_some(Toggled::True);
        nodes.push(row_node);
    }
    Some(PROJECT_PICKER_SEARCH_ID)
}

fn welcome_project_trigger(
    layout: &WorkspaceLayout,
    state: &ZodeAppState,
) -> Option<(String, Rect)> {
    if state.presentation.route != ShellRoute::Conversation || state.current_session.is_some() {
        return None;
    }
    let label = ProjectPicker::active_workspace_label(state)?;
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
    let title = EmptyState::welcome_title_layout(empty, Some(&label))?;
    Some((label, title.project?))
}

fn expand_and_clip(rect: Rect, clip: Rect, x: f32, y: f32) -> Rect {
    let min_x = (rect.min_x() - x).max(clip.min_x());
    let min_y = (rect.min_y() - y).max(clip.min_y());
    let max_x = (rect.max_x() + x).min(clip.max_x());
    let max_y = (rect.max_y() + y).min(clip.max_y());
    Rect::xywh(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}
