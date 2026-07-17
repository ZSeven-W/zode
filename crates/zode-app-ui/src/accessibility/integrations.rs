use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::{IntegrationsPage, ThreadTranscript, WorkspaceLayout, INTEGRATIONS_SEARCH_ID};

use super::{next_order, node, InteractionNode};

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
    for row in IntegrationsPage::catalog_section_layout(layout.primary_surface, state)
        .into_iter()
        .flat_map(|section| section.rows)
    {
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, layout.primary_surface)
        else {
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
        let Some(action_rect) =
            ThreadTranscript::clip_to_viewport(row.action_rect, layout.primary_surface)
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
}
