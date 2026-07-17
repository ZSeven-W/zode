use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use jian_widgets::Rect;
use zode_app_model::{
    BranchCatalogState, ConnectionState, ProjectPickerAnchor, ShellRoute, TaskLaunchMode,
    ZodeAppState,
};

use super::{next_order, node, visible_rect, InteractionNode};
use crate::{
    Composer, ComposerContextMenu, EmptyState, ProjectPicker, ProjectPickerViewState,
    ProjectSidebar, RectExt, WorkspaceLayout, COMPOSER_BRANCH_ID, COMPOSER_BRANCH_SEARCH_ID,
    COMPOSER_CONTEXT_MENU_SURFACE_ID, COMPOSER_LOCATION_ID, COMPOSER_PROJECT_ID, PROJECT_DETACH_ID,
    PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_TRIGGER_ID, TRANSCRIPT_COMPOSER_GAP,
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

pub(super) fn append_composer_context_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    if state.current_session_has_conversation() {
        return;
    }
    let label = ProjectPicker::active_workspace_label(state);
    let connection = composer_connection_label(state);
    let branch = composer_branch_label(state);
    let context = Composer::context_interaction_layout(
        layout.composer,
        state,
        label.as_deref(),
        Some(connection),
        branch,
    );
    if let Some(project) = context
        .project
        .filter(|chip| visible_rect(chip.action_rect))
    {
        let project_label = Composer::context_project_label(state, label.as_deref());
        nodes.push(node(
            COMPOSER_PROJECT_ID,
            project.action_rect,
            Role::Button,
            if label.is_some() {
                "更改此任务的项目"
            } else {
                "选择项目"
            },
            label
                .is_some()
                .then(|| project_label.map(str::to_owned))
                .flatten(),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    if let Some(detach) = context.detach.filter(|rect| visible_rect(*rect)) {
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
    if let Some(location) = context
        .location
        .filter(|chip| visible_rect(chip.action_rect))
    {
        nodes.push(node(
            COMPOSER_LOCATION_ID,
            location.action_rect,
            Role::Button,
            "选择任务的运行位置",
            Some(connection.into()),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
    if let Some(branch_rect) = context.branch.filter(|chip| visible_rect(chip.action_rect)) {
        nodes.push(node(
            COMPOSER_BRANCH_ID,
            branch_rect.action_rect,
            Role::Button,
            "切换任务分支",
            branch.map(str::to_owned),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

pub(super) fn append_picker_overlay(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
    picker: &ProjectPickerViewState,
) -> Option<crate::WidgetId> {
    let trigger = match state.project_picker.anchor {
        ProjectPickerAnchor::Welcome => welcome_project_trigger(layout, state)?.1,
        ProjectPickerAnchor::Composer => composer_project_trigger(layout, state)?,
        ProjectPickerAnchor::Sidebar => {
            ProjectSidebar::brand_search_rect(layout.primary_sidebar_content)
        }
    };
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
    for row in std::iter::once(picker_layout.new_project).chain(picker_layout.projectless) {
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

pub(super) fn append_composer_context_overlay(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) -> Option<crate::WidgetId> {
    if state.presentation.route != ShellRoute::Conversation || state.current_session.is_some() {
        return None;
    }
    let project = ProjectPicker::active_workspace_label(state);
    let connection = composer_connection_label(state);
    let branch = composer_branch_label(state);
    let context = Composer::context_interaction_layout(
        layout.composer,
        state,
        project.as_deref(),
        Some(connection),
        branch,
    );
    let menu = ComposerContextMenu::layout(layout.viewport, context, state)?;
    nodes.push(node(
        COMPOSER_CONTEXT_MENU_SURFACE_ID,
        menu.surface,
        Role::Menu,
        match menu.kind {
            zode_app_model::ComposerContextMenu::Location => "任务运行位置",
            zode_app_model::ComposerContextMenu::Branch => "任务分支",
        },
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    if let Some(search) = menu.search {
        nodes.push(node(
            COMPOSER_BRANCH_SEARCH_ID,
            search,
            Role::SearchInput,
            "搜索分支",
            Some(state.composer.branch_picker.query.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
    }
    for row in menu.rows.iter().chain(menu.create.iter()) {
        let mut item = node(
            row.id,
            row.rect,
            Role::MenuItem,
            &row.label,
            row.secondary.clone(),
            if row.enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            if row.enabled {
                next_order(focus_order)
            } else {
                None
            },
            if row.enabled {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        item.toggled = row.selected.then_some(Toggled::True);
        item.disabled = !row.enabled;
        nodes.push(item);
    }
    if menu.search.is_some() {
        Some(COMPOSER_BRANCH_SEARCH_ID)
    } else {
        menu.rows.iter().find(|row| row.enabled).map(|row| row.id)
    }
}

fn composer_project_trigger(layout: &WorkspaceLayout, state: &ZodeAppState) -> Option<Rect> {
    if state.presentation.route != ShellRoute::Conversation || state.current_session.is_some() {
        return None;
    }
    let label = ProjectPicker::active_workspace_label(state);
    let connection = composer_connection_label(state);
    let branch = composer_branch_label(state);
    Composer::context_interaction_layout(
        layout.composer,
        state,
        label.as_deref(),
        Some(connection),
        branch,
    )
    .project
    .map(|chip| chip.rect)
}

fn composer_connection_label(state: &ZodeAppState) -> &'static str {
    match state.composer.launch_mode {
        TaskLaunchMode::Worktree => "新工作树",
        TaskLaunchMode::Local => match state.host.connection {
            ConnectionState::Local => "本地",
            ConnectionState::Connecting => "连接中",
            ConnectionState::Unavailable => "不可用",
        },
    }
}

fn composer_branch_label(state: &ZodeAppState) -> Option<&str> {
    let workspace = state.active_available_workspace()?;
    if let Some(selected) = state.composer.selected_branch.as_deref() {
        return Some(selected);
    }
    if let BranchCatalogState::Ready(catalog) = &state.composer.branch_picker.catalog {
        if &catalog.workspace_uri == workspace && !catalog.current.trim().is_empty() {
            return Some(catalog.current.as_str());
        }
    }
    state
        .threads
        .iter()
        .filter(|thread| state.project_workspace_for_thread(thread) == Some(workspace))
        .filter_map(|thread| {
            let context = state
                .presentation
                .sessions
                .get(&thread.session)?
                .context
                .ready()?;
            if &context.workspace_uri != workspace {
                return None;
            }
            let branch = context
                .branch
                .as_deref()
                .filter(|branch| !branch.trim().is_empty())?;
            Some((thread.updated_at_ms, branch))
        })
        .max_by_key(|(updated_at_ms, _)| *updated_at_ms)
        .map(|(_, branch)| branch)
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
