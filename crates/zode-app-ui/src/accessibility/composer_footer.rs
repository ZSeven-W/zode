use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::{ComposerFooterMenu, ShellRoute, ZodeAppState};

use super::{next_order, node, visible_rect, InteractionNode};
use crate::{
    Composer, ComposerFooterMenuWidget, WidgetId, WorkspaceLayout, COMPOSER_ADD_ID,
    COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_MODEL_ID, COMPOSER_PERMISSION_ID,
};

pub(super) fn append_composer_footer_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let input = Composer::layout_for_state(layout.composer, state).input;
    if state.presentation.route != ShellRoute::Conversation || !visible_rect(input) {
        return;
    }
    let footer = ComposerFooterMenuWidget::trigger_layout(input, state);
    for (id, rect, name, value) in [
        (COMPOSER_ADD_ID, footer.add, "添加内容", None),
        (
            COMPOSER_PERMISSION_ID,
            footer.permission,
            "选择权限",
            Some(state.composer.sandbox_label.clone()),
        ),
        (
            COMPOSER_MODEL_ID,
            footer.model,
            if state.provider_setup_required {
                "配置 Codex"
            } else {
                "选择模型和推理强度"
            },
            state.composer.model.clone(),
        ),
    ] {
        if !visible_rect(rect) {
            continue;
        }
        nodes.push(node(
            id,
            rect,
            Role::Button,
            name,
            value,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

pub(super) fn append_composer_footer_overlay(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) -> Option<WidgetId> {
    if state.presentation.route != ShellRoute::Conversation {
        return None;
    }
    let input = Composer::layout_for_state(layout.composer, state).input;
    let menu = ComposerFooterMenuWidget::layout(layout.viewport, input, state)?;
    nodes.push(node(
        COMPOSER_FOOTER_MENU_SURFACE_ID,
        menu.surface,
        Role::Menu,
        match menu.kind {
            ComposerFooterMenu::Add => "添加内容",
            ComposerFooterMenu::Permission => "权限",
            ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed => "模型设置",
        },
        None,
        Vec::new(),
        None,
        CursorHint::Default,
    ));
    for row in &menu.rows {
        let mut item = node(
            row.id,
            row.rect,
            Role::MenuItem,
            &row.label,
            row.detail.clone(),
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
        item.disabled = !row.enabled;
        item.toggled = row.selected.then_some(Toggled::True);
        nodes.push(item);
    }
    menu.rows.iter().find(|row| row.enabled).map(|row| row.id)
}
