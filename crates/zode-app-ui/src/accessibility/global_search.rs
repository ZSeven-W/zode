use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use super::{next_order, node, InteractionNode};
use crate::{
    GlobalSearch, GlobalSearchViewState, WorkspaceLayout, GLOBAL_SEARCH_INPUT_ID,
    GLOBAL_SEARCH_SCRIM_ID, GLOBAL_SEARCH_SURFACE_ID,
};

pub(super) fn append_global_search_overlay(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
    view: &GlobalSearchViewState,
) -> Option<crate::WidgetId> {
    let search = GlobalSearch::layout(layout.viewport, state, view)?;
    nodes.push(node(
        GLOBAL_SEARCH_SCRIM_ID,
        search.scrim,
        Role::Button,
        "关闭全局搜索",
        None,
        vec![Action::Click],
        None,
        CursorHint::Default,
    ));
    // This node blocks blank-surface pointer hits from reaching the scrim;
    // keyboard focus remains on the search input below.
    nodes.push(node(
        GLOBAL_SEARCH_SURFACE_ID,
        search.surface,
        Role::Dialog,
        "全局搜索",
        None,
        vec![Action::Focus],
        None,
        CursorHint::Default,
    ));
    nodes.push(node(
        GLOBAL_SEARCH_INPUT_ID,
        search.search,
        Role::SearchInput,
        "搜索任务或运行命令",
        Some(view.query.clone()),
        vec![Action::Focus, Action::SetValue],
        next_order(focus_order),
        CursorHint::Text,
    ));
    for row in search.thread_rows.iter().chain(search.action_rows.iter()) {
        nodes.push(node(
            row.id,
            row.rect,
            Role::MenuItem,
            &row.label,
            row.detail.clone(),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    Some(GLOBAL_SEARCH_INPUT_ID)
}
