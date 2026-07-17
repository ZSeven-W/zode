use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::{
    IntegrationsPage, PluginDetailBody, ThreadTranscript, WorkspaceLayout,
    INTEGRATIONS_ADD_PLUGIN_ID, INTEGRATIONS_SEARCH_ID, PLUGIN_ADD_CANCEL_ID,
    PLUGIN_ADD_REFERENCE_INPUT_ID, PLUGIN_ADD_SPEC_INPUT_ID, PLUGIN_ADD_SUBMIT_ID,
    PLUGIN_DETAIL_CHECK_UPDATE_ID, PLUGIN_DETAIL_CLOSE_ID, PLUGIN_DETAIL_TRUST_ALL_ID,
    PLUGIN_DETAIL_TRUST_CANCEL_ID, PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID,
    PLUGIN_DETAIL_UNINSTALL_CANCEL_ID, PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID,
    PLUGIN_DETAIL_UNINSTALL_ID,
};

use super::{next_order, node, visible_rect, InteractionNode, INTEGRATIONS_ROOT_ID};

pub(super) fn append_integration_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let page = IntegrationsPage::layout(layout.primary_surface, state);
    for tab in page.tabs {
        let Some(rect) = ThreadTranscript::clip_to_viewport(tab.rect, layout.primary_surface)
        else {
            continue;
        };
        let mut tab_node = node(
            tab.id,
            rect,
            Role::Tab,
            tab.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        tab_node.toggled = Some(Toggled::from(tab.selected));
        nodes.push(tab_node);
    }
    if let Some(rect) = ThreadTranscript::clip_to_viewport(page.search, layout.primary_surface) {
        nodes.push(node(
            INTEGRATIONS_SEARCH_ID,
            rect,
            Role::SearchInput,
            "搜索插件或技能",
            Some(state.presentation.integration_search.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
    }
    for scope in page.scopes {
        let Some(rect) = ThreadTranscript::clip_to_viewport(scope.rect, layout.primary_surface)
        else {
            continue;
        };
        let mut scope_node = node(
            scope.id,
            rect,
            Role::RadioButton,
            scope.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        scope_node.toggled = Some(Toggled::from(scope.selected));
        nodes.push(scope_node);
    }
    for icon in IntegrationsPage::installed_icon_layout(layout.primary_surface, state) {
        let Some(rect) = ThreadTranscript::clip_to_viewport(icon.rect, layout.primary_surface)
        else {
            continue;
        };
        nodes.push(node(
            icon.id,
            rect,
            Role::Image,
            &format!("已安装 {}", icon.name),
            Some(icon.status.into()),
            Vec::new(),
            None,
            CursorHint::Default,
        ));
    }
    if visible_rect(page.catalog) {
        let max_scroll = IntegrationsPage::max_scroll_offset(layout.primary_surface, state);
        let scroll_offset = IntegrationsPage::scroll_offset(layout.primary_surface, state);
        let mut actions = Vec::new();
        if scroll_offset > 0.0 {
            actions.push(Action::ScrollUp);
        }
        if scroll_offset < max_scroll {
            actions.push(Action::ScrollDown);
        }
        nodes.push(node(
            INTEGRATIONS_ROOT_ID,
            page.catalog,
            Role::ScrollView,
            "集成目录",
            None,
            actions,
            None,
            CursorHint::Default,
        ));
    }
    for row in IntegrationsPage::catalog_section_layout(layout.primary_surface, state)
        .into_iter()
        .flat_map(|section| section.rows)
    {
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, page.catalog) else {
            continue;
        };
        nodes.push(node(
            row.id,
            rect,
            Role::ListItem,
            &format!("{}，{}", row.name, row.status),
            Some(row.description),
            Vec::new(),
            None,
            CursorHint::Default,
        ));
        let Some(action_rect) = ThreadTranscript::clip_to_viewport(row.action_rect, page.catalog)
        else {
            continue;
        };
        let mut action = node(
            row.action_id,
            action_rect,
            Role::Button,
            &format!("{} {}", row.action_label, row.name),
            row.action_reason.map(str::to_owned),
            if row.action_enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            row.action_enabled
                .then(|| next_order(focus_order))
                .flatten(),
            if row.action_enabled {
                CursorHint::Pointer
            } else {
                CursorHint::NotAllowed
            },
        );
        action.disabled = !row.action_enabled;
        nodes.push(action);
    }

    if let Some(rect) =
        ThreadTranscript::clip_to_viewport(page.add_plugin_button, layout.primary_surface)
    {
        if page.add_plugin_button.size.x > 0.0 {
            nodes.push(node(
                INTEGRATIONS_ADD_PLUGIN_ID,
                rect,
                Role::Button,
                "添加插件",
                None,
                vec![Action::Click, Action::Focus],
                next_order(focus_order),
                CursorHint::Pointer,
            ));
        }
    }
    for row in IntegrationsPage::plugin_row_layout(layout.primary_surface, state) {
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, layout.primary_surface)
        else {
            continue;
        };
        nodes.push(node(
            row.id,
            rect,
            Role::Button,
            &format!("{}，{}", row.repo, row.trust_label),
            Some(row.reference),
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }

    if let Some(form) = IntegrationsPage::plugin_add_form_layout(layout.primary_surface, state) {
        nodes.push(node(
            PLUGIN_ADD_SPEC_INPUT_ID,
            form.spec_input,
            Role::TextInput,
            "插件仓库",
            Some(state.presentation.plugin_add.spec.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
        nodes.push(node(
            PLUGIN_ADD_REFERENCE_INPUT_ID,
            form.reference_input,
            Role::TextInput,
            "分支或提交",
            Some(state.presentation.plugin_add.reference.clone()),
            vec![Action::Focus, Action::SetValue],
            next_order(focus_order),
            CursorHint::Text,
        ));
        nodes.push(node(
            PLUGIN_ADD_CANCEL_ID,
            form.cancel,
            Role::Button,
            "取消",
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
        let mut submit = node(
            PLUGIN_ADD_SUBMIT_ID,
            form.submit,
            Role::Button,
            "安装",
            None,
            if form.submit_enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            form.submit_enabled
                .then(|| next_order(focus_order))
                .flatten(),
            if form.submit_enabled {
                CursorHint::Pointer
            } else {
                CursorHint::NotAllowed
            },
        );
        submit.disabled = !form.submit_enabled;
        nodes.push(submit);
    }

    if let Some(detail) = IntegrationsPage::plugin_detail_layout(layout.primary_surface, state) {
        nodes.push(node(
            PLUGIN_DETAIL_CLOSE_ID,
            detail.close,
            Role::Button,
            "关闭",
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
        match &detail.body {
            PluginDetailBody::Overview {
                capabilities,
                check_update,
                uninstall,
                ..
            } => {
                for row in capabilities {
                    if let Some(action) = row.toggle_action {
                        let label = if row.gated {
                            "审查"
                        } else if action.currently_enabled {
                            "停用"
                        } else {
                            "启用"
                        };
                        nodes.push(node(
                            action.widget_id,
                            action_rect_for(row.rect),
                            Role::Button,
                            &format!("{label} {}", row.capability.label),
                            None,
                            vec![Action::Click, Action::Focus],
                            next_order(focus_order),
                            CursorHint::Pointer,
                        ));
                    }
                }
                nodes.push(node(
                    PLUGIN_DETAIL_CHECK_UPDATE_ID,
                    *check_update,
                    Role::Button,
                    "检查更新",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
                nodes.push(node(
                    PLUGIN_DETAIL_UNINSTALL_ID,
                    *uninstall,
                    Role::Button,
                    "删除",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            PluginDetailBody::ConfirmUninstall { confirm, cancel } => {
                nodes.push(node(
                    PLUGIN_DETAIL_UNINSTALL_CANCEL_ID,
                    *cancel,
                    Role::Button,
                    "取消",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
                nodes.push(node(
                    PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID,
                    *confirm,
                    Role::Button,
                    "确认删除",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            PluginDetailBody::Uninstalling => {}
            PluginDetailBody::TrustReview {
                items,
                trust_all,
                grant_selected,
                grant_selected_enabled,
                cancel,
                ..
            } => {
                for item in items {
                    let mut checkbox = node(
                        item.checkbox_id,
                        item.rect,
                        Role::CheckBox,
                        &item.key,
                        Some(item.content.clone()),
                        vec![Action::Click, Action::Focus],
                        next_order(focus_order),
                        CursorHint::Pointer,
                    );
                    checkbox.toggled = Some(Toggled::from(item.selected));
                    nodes.push(checkbox);
                }
                nodes.push(node(
                    PLUGIN_DETAIL_TRUST_CANCEL_ID,
                    *cancel,
                    Role::Button,
                    "取消",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
                let mut grant_selected_node = node(
                    PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID,
                    *grant_selected,
                    Role::Button,
                    "信任所选",
                    None,
                    if *grant_selected_enabled {
                        vec![Action::Click, Action::Focus]
                    } else {
                        Vec::new()
                    },
                    grant_selected_enabled
                        .then(|| next_order(focus_order))
                        .flatten(),
                    if *grant_selected_enabled {
                        CursorHint::Pointer
                    } else {
                        CursorHint::NotAllowed
                    },
                );
                grant_selected_node.disabled = !grant_selected_enabled;
                nodes.push(grant_selected_node);
                nodes.push(node(
                    PLUGIN_DETAIL_TRUST_ALL_ID,
                    *trust_all,
                    Role::Button,
                    "全部信任",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
    }
}

/// The capability row's small trailing action button, matching
/// `plugin_detail::paint_capability_row`'s own geometry - kept in sync
/// manually since the paint code computes this rect locally rather than
/// exposing it on `CapabilityRowLayout`.
fn action_rect_for(row_rect: jian_widgets::Rect) -> jian_widgets::Rect {
    jian_widgets::Rect::xywh(
        row_rect.origin.x + row_rect.size.x - 88.0,
        row_rect.origin.y + (row_rect.size.y - 24.0) / 2.0,
        88.0,
        24.0,
    )
}
