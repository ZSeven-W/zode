use accesskit::{Action, Role};
use jian_core::CursorHint;
use jian_widgets::Rect;
use zode_app_model::{EnvironmentSectionKind, ZodeAppState};
use zode_node_protocol::{BackgroundProcessStatus, SessionLocator};

use crate::{
    EnvironmentPanel, EnvironmentSectionLayout, PinnedSummaryMode, ThreadTranscript,
    WorkspaceLayout, ENVIRONMENT_CLOSE_ID, ENVIRONMENT_PANEL_ID,
};

use super::{clip_nodes_since, next_order, node, visible_rect, InteractionNode};

/// Width reserved at a BackgroundProcesses row's trailing edge for the
/// "查看输出" (view output) hit target - mirrors the icon reservation in
/// `widgets::environment::row::paint_background_process`. Everything to
/// its left is the row's "停止" (stop) target.
const VIEW_OUTPUT_HIT_WIDTH: f32 = 28.0;

/// `EnvironmentEntry::id` prefix for a BackgroundProcesses row - duplicated
/// from `widgets::environment::row::BACKGROUND_PROCESS_ID_PREFIX` (that
/// constant is private to the widgets module tree; this is one string
/// literal, not worth widening its visibility for).
fn background_process_id(entry_id: &str) -> Option<&str> {
    entry_id.strip_prefix("bg:")
}

pub(super) fn append_environment_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let nodes_start = nodes.len();
    let panel = EnvironmentPanel::layout(layout.pinned_summary_content, state);
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
        for section in &panel.sections {
            let Some(rect) = ThreadTranscript::clip_to_viewport(section.rect, panel.content) else {
                continue;
            };
            // The Subagents section's one compact row opens the M2 dedicated
            // panel (see `docs/proposals/subagent-panel-m2.md`) - every other
            // section here is purely informational, so only this kind ever
            // carries a click action.
            let opens_subagents_panel = section.section.kind == EnvironmentSectionKind::Subagents
                && !section.section.entries.is_empty();
            nodes.push(node(
                EnvironmentPanel::section_widget_id(session, section.section.kind),
                rect,
                Role::Group,
                &EnvironmentPanel::section_accessibility_name(section),
                None,
                if opens_subagents_panel {
                    vec![Action::Click, Action::Focus]
                } else {
                    Vec::new()
                },
                opens_subagents_panel
                    .then(|| next_order(focus_order))
                    .flatten(),
                if opens_subagents_panel {
                    CursorHint::Pointer
                } else {
                    CursorHint::Default
                },
            ));
            if section.section.kind == EnvironmentSectionKind::BackgroundProcesses {
                append_background_process_nodes(
                    nodes,
                    section,
                    panel.content,
                    session,
                    state,
                    focus_order,
                );
            }
        }
        if let Some(row) = &panel.pull_request_row {
            if !row.painted_by_action {
                if let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, panel.content) {
                    let value = row
                        .entry
                        .value
                        .as_deref()
                        .map(|value| format!("{}：{value}", row.entry.label))
                        .unwrap_or_else(|| row.entry.label.clone());
                    nodes.push(node(
                        EnvironmentPanel::pull_request_widget_id(session),
                        rect,
                        Role::Group,
                        &value,
                        None,
                        Vec::new(),
                        None,
                        CursorHint::Default,
                    ));
                }
            }
        }
    }
    if layout.pinned_summary == PinnedSummaryMode::Overlay {
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
    }
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
    clip_nodes_since(nodes, nodes_start, layout.context_panel);
}

/// Appends the stop/view-output hit targets for every BackgroundProcesses
/// row - split from the row's own (informational) Group node above since
/// these carry real click actions and per-state disabled tooltips. Enabled
/// state and tooltips are resolved against the live process list rather
/// than baked into layout, so a process that transitions between frames
/// (e.g. stopped externally) is never stale here.
fn append_background_process_nodes(
    nodes: &mut Vec<InteractionNode>,
    section: &EnvironmentSectionLayout,
    content: Rect,
    session: &SessionLocator,
    state: &ZodeAppState,
    focus_order: &mut u32,
) {
    let Some(presentation) = state.current_session_presentation() else {
        return;
    };
    for row in &section.rows {
        let Some(process_id) = background_process_id(&row.entry.id) else {
            continue;
        };
        let Some(process) = presentation
            .background_processes
            .iter()
            .find(|process| process.id == process_id)
        else {
            continue;
        };
        let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, content) else {
            continue;
        };
        let stop_width = (rect.size.x - VIEW_OUTPUT_HIT_WIDTH).max(0.0);
        let stop_rect = Rect::xywh(rect.origin.x, rect.origin.y, stop_width, rect.size.y);
        let view_output_rect = Rect::xywh(
            rect.origin.x + stop_width,
            rect.origin.y,
            rect.size.x - stop_width,
            rect.size.y,
        );

        let armed = presentation.armed_stop_process_id.as_deref() == Some(process_id);
        let stop_running = process.status == BackgroundProcessStatus::Running;
        let stop_label = if armed { "确认停止" } else { "停止" };
        let stop_disabled_reason = (!stop_running).then(|| {
            format!(
                "{}：无法停止",
                background_process_status_label(process.status)
            )
        });
        let mut stop_node = node(
            EnvironmentPanel::background_process_stop_widget_id(session, process_id),
            stop_rect,
            Role::Button,
            stop_label,
            stop_disabled_reason,
            if stop_running {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            stop_running.then(|| next_order(focus_order)).flatten(),
            if stop_running {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        stop_node.disabled = !stop_running;
        nodes.push(stop_node);

        let can_view_output = process.tool_call_id.is_some();
        let mut view_output_node = node(
            EnvironmentPanel::background_process_view_output_widget_id(session, process_id),
            view_output_rect,
            Role::Button,
            "查看输出",
            (!can_view_output).then(|| "无法跳转".to_owned()),
            if can_view_output {
                vec![Action::Click, Action::Focus]
            } else {
                Vec::new()
            },
            can_view_output.then(|| next_order(focus_order)).flatten(),
            if can_view_output {
                CursorHint::Pointer
            } else {
                CursorHint::Default
            },
        );
        view_output_node.disabled = !can_view_output;
        nodes.push(view_output_node);
    }
}

const fn background_process_status_label(status: BackgroundProcessStatus) -> &'static str {
    match status {
        BackgroundProcessStatus::Starting => "正在启动",
        BackgroundProcessStatus::Running => "运行中",
        BackgroundProcessStatus::Stopping => "正在停止",
        BackgroundProcessStatus::Stopped => "已停止",
        BackgroundProcessStatus::NotFound => "未找到",
    }
}
