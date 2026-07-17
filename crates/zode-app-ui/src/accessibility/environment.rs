use accesskit::{Action, Role};
use jian_core::CursorHint;
use zode_app_model::ZodeAppState;

use crate::{
    EnvironmentPanel, ThreadTranscript, WorkspaceLayout, ENVIRONMENT_CLOSE_ID, ENVIRONMENT_PANEL_ID,
};

use super::{next_order, node, visible_rect, InteractionNode};

pub(super) fn append_environment_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let panel = EnvironmentPanel::layout(layout.context_panel, state);
    if !visible_rect(panel.card) {
        return;
    }
    nodes.push(node(
        ENVIRONMENT_PANEL_ID,
        panel.card,
        Role::Group,
        "置顶摘要面板",
        None,
        vec![Action::Click],
        None,
        CursorHint::Default,
    ));
    if let Some(session) = state.current_session.as_ref() {
        for section in panel.sections.iter().filter(|section| !section.footer) {
            let Some(rect) = ThreadTranscript::clip_to_viewport(section.rect, panel.content) else {
                continue;
            };
            nodes.push(node(
                EnvironmentPanel::section_widget_id(session, section.section.kind),
                rect,
                Role::Group,
                &EnvironmentPanel::section_accessibility_name(section),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            ));
        }
    }
    nodes.push(node(
        ENVIRONMENT_CLOSE_ID,
        panel.close_button,
        Role::Button,
        "关闭置顶摘要面板",
        None,
        vec![Action::Click, Action::Focus],
        next_order(focus_order),
        CursorHint::Pointer,
    ));
    for action in panel
        .repository_actions
        .iter()
        .filter(|action| visible_rect(action.rect))
    {
        let enabled = action.action.enabled();
        let mut interaction = node(
            action.id,
            action.rect,
            Role::Button,
            action.action.kind.label(),
            action
                .action
                .unavailable_reason
                .map(|reason| reason.message().to_owned()),
            if enabled {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            enabled.then(|| next_order(focus_order)).flatten(),
            if enabled {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        interaction.disabled = !enabled;
        nodes.push(interaction);
    }
}
