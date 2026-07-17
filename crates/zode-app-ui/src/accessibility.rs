use accesskit::{Action, Node, NodeId, Role, Toggled, Tree, TreeId, TreeUpdate};
use jian_core::CursorHint;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    ComingSoonFeature, IntegrationsTab, PreviewState, SecondaryPane, ShellRoute, ZodeAppState,
};

use crate::{
    composer_queue_reserved_height, Composer, DocumentPreview, Insets, PinnedSummaryMode,
    ProjectPickerViewState, RectExt, ReviewPanel, SettingsPanel, TerminalSecondaryPanel,
    ThreadTranscript, UnavailableSecondaryPanel, WorkspaceLayout, COMPOSER_ATTACHMENT_H,
    COMPOSER_H, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID, INTEGRATIONS_PLUGINS_TAB_ID,
    INTEGRATIONS_SKILLS_TAB_ID, SECONDARY_PANE_BREAKPOINT, TERMINAL_SECONDARY_CLOSE_ID,
    UNAVAILABLE_SECONDARY_CLOSE_ID,
};

mod composer_footer;
mod empty_state;
mod environment;
mod header;
mod ids;
mod integrations;
mod project_picker;
mod queue;
mod settings;
mod sidebar;
mod transcript;

use composer_footer::{append_composer_footer_nodes, append_composer_footer_overlay};
use empty_state::append_empty_suggestion_nodes;
use environment::append_environment_nodes;
use header::{append_header_menu_nodes, append_header_nodes, append_panel_picker_nodes};
pub(crate) use ids::stable_widget_id;
use integrations::append_integration_nodes;
use project_picker::{
    append_composer_context_nodes, append_composer_context_overlay, append_picker_overlay,
    append_welcome_project_trigger,
};
use queue::{append_queue_menu_nodes, append_queue_nodes};
use settings::append_settings_nodes;
use sidebar::{append_sidebar_menu_nodes, append_sidebar_nodes};
use transcript::append_transcript_nodes;

pub const SIDEBAR_ID: WidgetId = WidgetId(1);
pub const NEW_SESSION_ID: WidgetId = WidgetId(2);
pub const SCHEDULED_NAV_ID: WidgetId = WidgetId(3);
pub const WORKFLOWS_NAV_ID: WidgetId = SCHEDULED_NAV_ID;
pub const PLUGINS_NAV_ID: WidgetId = WidgetId(4);
pub const SITES_NAV_ID: WidgetId = WidgetId(5);
pub const OPENPENCIL_NAV_ID: WidgetId = SITES_NAV_ID;
pub const PULL_REQUESTS_NAV_ID: WidgetId = WidgetId(6);
pub const BROWSER_NAV_ID: WidgetId = PULL_REQUESTS_NAV_ID;
pub const CHATS_NAV_ID: WidgetId = WidgetId(7);
pub const SETTINGS_ROOT_ID: WidgetId = WidgetId(8);
pub const SETTINGS_NAV_ID: WidgetId = WidgetId(9);
pub const HELP_ID: WidgetId = WidgetId(10);
pub const COMPOSER_ID: WidgetId = WidgetId(20);
pub const SEND_ID: WidgetId = WidgetId(21);
pub const TERMINAL_ID: WidgetId = WidgetId(30);
pub const THEME_SYSTEM_ID: WidgetId = WidgetId(40);
pub const THEME_LIGHT_ID: WidgetId = WidgetId(41);
pub const THEME_DARK_ID: WidgetId = WidgetId(42);
pub const REDUCED_MOTION_ID: WidgetId = WidgetId(43);
pub const HIGH_CONTRAST_ID: WidgetId = WidgetId(44);
pub const HEADER_ENVIRONMENT_ID: WidgetId = WidgetId(60);
pub const HEADER_REVIEW_ID: WidgetId = WidgetId(61);
pub const REVIEW_CLOSE_ID: WidgetId = WidgetId(102);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WidgetId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct InteractionNode {
    pub id: WidgetId,
    pub rect: Rect,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    pub actions: Vec<Action>,
    pub focus_order: Option<u32>,
    pub cursor: CursorHint,
    pub toggled: Option<Toggled>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Forward,
    Backward,
}

/// One immutable interaction snapshot consumed by paint, hit testing and a11y.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSnapshot {
    pub layout: WorkspaceLayout,
    pub nodes: Vec<InteractionNode>,
    pub focused: Option<WidgetId>,
}

impl WorkspaceSnapshot {
    pub fn build(state: &ZodeAppState, width: f32, height: f32, insets: Insets) -> Self {
        let picker = ProjectPickerViewState {
            open: state.project_picker.open,
            query: state.project_picker.search.clone(),
        };
        Self::build_internal(state, width, height, insets, Some(&picker))
    }

    pub fn build_with_project_picker(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: &ProjectPickerViewState,
    ) -> Self {
        Self::build_internal(state, width, height, insets, Some(project_picker))
    }

    fn build_internal(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: Option<&ProjectPickerViewState>,
    ) -> Self {
        let route = state.presentation.route;
        let queue_count = state
            .current_session
            .as_ref()
            .and_then(|session| state.message_queues.get(session))
            .map_or(0, |queue| queue.items.len());
        let composer_height = COMPOSER_H
            + if route == ShellRoute::Conversation && !state.composer.attachments.is_empty() {
                COMPOSER_ATTACHMENT_H
            } else {
                0.0
            }
            + if route == ShellRoute::Conversation {
                composer_queue_reserved_height(queue_count)
            } else {
                0.0
            };
        let auto_pinned_summary = route == ShellRoute::Conversation
            && state.current_session.is_some()
            && state.presentation.secondary_pane.is_none()
            && !state.presentation.pinned_summary_auto_hidden
            && width >= SECONDARY_PANE_BREAKPOINT;
        let layout_secondary = if auto_pinned_summary {
            Some(SecondaryPane::Environment)
        } else {
            state.presentation.secondary_pane
        };
        let layout = WorkspaceLayout::compute_presentation_with_composer_height(
            width,
            height,
            insets,
            route,
            layout_secondary,
            composer_height,
        );
        let mut nodes = Vec::new();
        let mut focus_order = 0;
        let split_fallback = route == ShellRoute::Conversation
            && matches!(
                state.presentation.secondary_pane,
                Some(
                    SecondaryPane::Review
                        | SecondaryPane::DocumentPreview
                        | SecondaryPane::Terminal
                        | SecondaryPane::Browser
                        | SecondaryPane::Files
                        | SecondaryPane::SideTask
                )
            )
            && !visible_rect(layout.review_panel)
            && visible_rect(layout.primary_surface);

        if visible_rect(layout.sidebar) && !matches!(route, ShellRoute::Settings(_)) {
            append_sidebar_nodes(&mut nodes, &layout, &mut focus_order, state);
        }

        let focused = match route {
            ShellRoute::Settings(category) => {
                append_settings_nodes(&mut nodes, &layout, &mut focus_order, state);
                nodes
                    .iter()
                    .find(|node| node.id == SettingsPanel::category_widget_id(category))
                    .filter(|node| node.focus_order.is_some())
                    .or_else(|| nodes.iter().find(|node| node.focus_order.is_some()))
                    .map(|node| node.id)
            }
            ShellRoute::Terminal => {
                let rect = Rect::xywh(
                    layout.transcript.origin.x,
                    layout.transcript.origin.y,
                    layout.transcript.size.x,
                    layout.composer.max_y() - layout.transcript.origin.y,
                );
                if visible_rect(rect) {
                    nodes.push(node(
                        TERMINAL_ID,
                        rect,
                        Role::TextInput,
                        "终端",
                        None,
                        vec![Action::Focus],
                        next_order(&mut focus_order),
                        CursorHint::Text,
                    ));
                    Some(TERMINAL_ID)
                } else {
                    None
                }
            }
            ShellRoute::Integrations(tab) => {
                append_integration_nodes(&mut nodes, &layout, &mut focus_order, state);
                let selected = match tab {
                    IntegrationsTab::Plugins => INTEGRATIONS_PLUGINS_TAB_ID,
                    IntegrationsTab::Skills => INTEGRATIONS_SKILLS_TAB_ID,
                };
                nodes
                    .iter()
                    .any(|node| node.id == selected)
                    .then_some(selected)
            }
            ShellRoute::ComingSoon(feature) => {
                coming_soon_focus(feature).filter(|id| nodes.iter().any(|node| node.id == *id))
            }
            ShellRoute::Conversation => {
                if split_fallback {
                    None
                } else {
                    append_header_nodes(&mut nodes, &layout, &mut focus_order, state);
                    append_transcript_nodes(&mut nodes, &layout, &mut focus_order, state);
                    append_welcome_project_trigger(&mut nodes, &layout, &mut focus_order, state);
                    append_empty_suggestion_nodes(&mut nodes, &layout, &mut focus_order, state);
                    let composer_layout = Composer::layout_for_state(layout.composer, state);
                    append_queue_nodes(&mut nodes, &layout, &mut focus_order, state);
                    if let Some(attachment_strip) = composer_layout.attachments {
                        for attachment_layout in
                            Composer::attachment_layouts(attachment_strip, &state.composer)
                        {
                            let Some(attachment) = state
                                .composer
                                .attachments
                                .iter()
                                .find(|attachment| attachment.id == attachment_layout.id)
                            else {
                                continue;
                            };
                            let dimensions = match (attachment.width, attachment.height) {
                                (Some(width), Some(height)) => format!("{width}×{height}"),
                                _ => "尺寸未知".into(),
                            };
                            nodes.push(node(
                                Composer::attachment_widget_id(&attachment.id),
                                attachment_layout.rect,
                                Role::Image,
                                &format!("附件 {}", attachment.display_name),
                                Some(format!(
                                    "{}，{}，{} 字节",
                                    attachment.media_type, dimensions, attachment.byte_len
                                )),
                                Vec::new(),
                                None,
                                CursorHint::Default,
                            ));
                        }
                    }
                    append_composer_context_nodes(&mut nodes, &layout, &mut focus_order, state);
                    append_composer_footer_nodes(&mut nodes, &layout, &mut focus_order, state);
                    if visible_rect(composer_layout.input) {
                        nodes.push(node(
                            COMPOSER_ID,
                            composer_layout.input,
                            Role::TextInput,
                            "要求后续变更",
                            Some(state.composer.draft.clone()),
                            vec![Action::Focus, Action::SetValue],
                            next_order(&mut focus_order),
                            CursorHint::Text,
                        ));
                        let send_rect = Rect::xywh(
                            composer_layout.input.max_x() - 42.0,
                            composer_layout.input.max_y() - 38.0,
                            28.0,
                            28.0,
                        );
                        if let Some(send_rect) =
                            ThreadTranscript::clip_to_viewport(send_rect, composer_layout.input)
                        {
                            let busy = current_session_busy(state);
                            let enabled = busy || Composer::can_submit(&state.composer);
                            let mut send = node(
                                SEND_ID,
                                send_rect,
                                Role::Button,
                                if busy { "停止当前运行" } else { "发送" },
                                None,
                                if enabled {
                                    vec![Action::Click, Action::Focus]
                                } else {
                                    Vec::new()
                                },
                                if enabled {
                                    next_order(&mut focus_order)
                                } else {
                                    None
                                },
                                if enabled {
                                    CursorHint::Pointer
                                } else {
                                    CursorHint::Default
                                },
                            );
                            send.disabled = !enabled;
                            nodes.push(send);
                        }
                        append_queue_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
                        Some(COMPOSER_ID)
                    } else {
                        None
                    }
                }
            }
        };
        append_secondary_nodes(&mut nodes, &layout, &mut focus_order, state);
        if visible_rect(layout.sidebar) && !matches!(route, ShellRoute::Settings(_)) {
            append_sidebar_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
        }
        let header_overlay_focus = if route == ShellRoute::Conversation {
            let focus = append_header_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
            append_panel_picker_nodes(&mut nodes, &layout, &mut focus_order, state);
            focus
        } else {
            None
        };
        let mut focused = if split_fallback {
            let close_id = match state.presentation.secondary_pane {
                Some(SecondaryPane::DocumentPreview) => DOCUMENT_PREVIEW_CLOSE_ID,
                Some(SecondaryPane::Terminal) => TERMINAL_ID,
                Some(SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask) => {
                    UNAVAILABLE_SECONDARY_CLOSE_ID
                }
                Some(SecondaryPane::Review | SecondaryPane::Environment) | None => REVIEW_CLOSE_ID,
            };
            nodes
                .iter()
                .find(|node| node.id == close_id)
                .map(|node| node.id)
        } else {
            focused
        };
        if let Some(header_overlay_focus) = header_overlay_focus {
            focused = Some(header_overlay_focus);
        }
        if let Some(project_picker) = project_picker {
            if let Some(picker_focus) =
                append_picker_overlay(&mut nodes, &layout, &mut focus_order, state, project_picker)
            {
                focused = Some(picker_focus);
            }
        }
        if let Some(context_focus) =
            append_composer_context_overlay(&mut nodes, &layout, &mut focus_order, state)
        {
            focused = Some(context_focus);
        }
        if let Some(footer_focus) =
            append_composer_footer_overlay(&mut nodes, &layout, &mut focus_order, state)
        {
            focused = Some(footer_focus);
        }

        Self {
            layout,
            nodes,
            focused,
        }
    }

    pub fn node(&self, id: WidgetId) -> Option<&InteractionNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn hit_test(&self, point: Point2D) -> Option<WidgetId> {
        self.nodes
            .iter()
            .rev()
            .find(|node| !node.actions.is_empty() && node.rect.contains(point))
            .map(|node| node.id)
    }

    pub fn focusable_ids(&self) -> Vec<WidgetId> {
        let mut ordered = self
            .nodes
            .iter()
            .filter_map(|node| node.focus_order.map(|order| (order, node.id)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(order, _)| *order);
        ordered.into_iter().map(|(_, id)| id).collect()
    }

    pub fn move_focus(
        &self,
        current: Option<WidgetId>,
        direction: FocusDirection,
    ) -> Option<WidgetId> {
        let order = self.focusable_ids();
        if order.is_empty() {
            return None;
        }
        let current_index =
            current.and_then(|id| order.iter().position(|candidate| *candidate == id));
        let index = match (direction, current_index) {
            (FocusDirection::Forward, Some(index)) => (index + 1) % order.len(),
            (FocusDirection::Backward, Some(0)) => order.len() - 1,
            (FocusDirection::Backward, Some(index)) => index - 1,
            (FocusDirection::Forward, None) => 0,
            (FocusDirection::Backward, None) => order.len() - 1,
        };
        Some(order[index])
    }
}

fn current_session_busy(state: &ZodeAppState) -> bool {
    state
        .current_session
        .as_ref()
        .and_then(|session| state.transcripts.get(session))
        .is_some_and(|transcript| transcript.busy)
}

fn append_secondary_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    if state.presentation.route != ShellRoute::Conversation {
        return;
    }
    match state.presentation.secondary_pane {
        Some(SecondaryPane::Environment) if visible_rect(layout.context_panel) => {
            append_environment_nodes(nodes, layout, focus_order, state);
        }
        Some(SecondaryPane::Review) => {
            let panel_rect = if visible_rect(layout.review_panel) {
                layout.review_panel
            } else {
                layout.primary_surface
            };
            let rect = ReviewPanel::layout(panel_rect).close_button;
            if visible_rect(rect) {
                nodes.push(node(
                    REVIEW_CLOSE_ID,
                    rect,
                    Role::Button,
                    "关闭审查",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            for row in ReviewPanel::file_row_layouts(panel_rect, state) {
                if !visible_rect(row.rect) {
                    continue;
                }
                nodes.push(node(
                    row.id,
                    row.rect,
                    Role::Button,
                    &format!("预览文件 {}", row.path),
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
        Some(SecondaryPane::DocumentPreview) => {
            let panel_rect = if visible_rect(layout.review_panel) {
                layout.review_panel
            } else {
                layout.primary_surface
            };
            let preview = DocumentPreview::layout(panel_rect, state);
            if visible_rect(preview.close_button) {
                nodes.push(node(
                    DOCUMENT_PREVIEW_CLOSE_ID,
                    preview.close_button,
                    Role::Button,
                    "关闭文档预览",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            if let Some(rect) = preview.external_button.filter(|rect| visible_rect(*rect)) {
                nodes.push(node(
                    DOCUMENT_PREVIEW_EXTERNAL_ID,
                    rect,
                    Role::Button,
                    "在外部打开文件",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            if let Some(rect) = preview.retry_button.filter(|rect| visible_rect(*rect)) {
                nodes.push(node(
                    DOCUMENT_PREVIEW_RETRY_ID,
                    rect,
                    Role::Button,
                    "重试文档预览",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            let preview_state = DocumentPreview::current_state(state);
            let (name, value) = match preview_state {
                Some(PreviewState::Ready {
                    title,
                    content,
                    target,
                    ..
                }) => (
                    format!("文档预览：{title}"),
                    Some(format!(
                        "{}，{}",
                        target.relative_path,
                        preview_accessibility_excerpt(content)
                    )),
                ),
                Some(PreviewState::Failed { target, message }) => (
                    format!("文档预览失败：{}", target.relative_path),
                    Some(message.clone()),
                ),
                Some(PreviewState::Loading { target }) => {
                    (format!("正在加载文档：{}", target.relative_path), None)
                }
                None | Some(PreviewState::Idle) => ("文档预览".into(), None),
            };
            if visible_rect(preview.content) {
                nodes.push(node(
                    DOCUMENT_PREVIEW_CONTENT_ID,
                    preview.content,
                    Role::Document,
                    &name,
                    value,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
            }
        }
        Some(SecondaryPane::Terminal) => {
            let panel_rect = if visible_rect(layout.review_panel) {
                layout.review_panel
            } else {
                layout.primary_surface
            };
            let panel = TerminalSecondaryPanel::layout(panel_rect);
            if visible_rect(panel.close_button) {
                nodes.push(node(
                    TERMINAL_SECONDARY_CLOSE_ID,
                    panel.close_button,
                    Role::Button,
                    "关闭终端面板",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            if visible_rect(panel.content) {
                nodes.push(node(
                    TERMINAL_ID,
                    panel.content,
                    Role::TextInput,
                    "终端",
                    state.terminal.unavailable_reason.clone(),
                    vec![Action::Focus],
                    next_order(focus_order),
                    CursorHint::Text,
                ));
            }
        }
        Some(pane @ (SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask)) => {
            let panel_rect = if visible_rect(layout.review_panel) {
                layout.review_panel
            } else {
                layout.primary_surface
            };
            let close = UnavailableSecondaryPanel::close_button(panel_rect);
            nodes.push(node(
                UNAVAILABLE_SECONDARY_CLOSE_ID,
                close,
                Role::Button,
                &format!("关闭{}面板", UnavailableSecondaryPanel::title(pane)),
                None,
                vec![Action::Click, Action::Focus],
                next_order(focus_order),
                CursorHint::Pointer,
            ));
            nodes.push(node(
                WidgetId(112),
                panel_rect,
                Role::Group,
                UnavailableSecondaryPanel::title(pane),
                Some(UnavailableSecondaryPanel::message(pane).into()),
                Vec::new(),
                None,
                CursorHint::Default,
            ));
        }
        None if layout.pinned_summary != PinnedSummaryMode::Hidden
            && visible_rect(layout.context_panel) =>
        {
            append_environment_nodes(nodes, layout, focus_order, state);
        }
        Some(SecondaryPane::Environment) | None => {}
    }
}

const fn coming_soon_focus(feature: ComingSoonFeature) -> Option<WidgetId> {
    Some(match feature {
        ComingSoonFeature::ScheduledTasks => SCHEDULED_NAV_ID,
        ComingSoonFeature::Sites => SITES_NAV_ID,
        ComingSoonFeature::PullRequests => PULL_REQUESTS_NAV_ID,
        ComingSoonFeature::Chats => CHATS_NAV_ID,
        ComingSoonFeature::Help => HELP_ID,
    })
}

fn visible_rect(rect: Rect) -> bool {
    rect.size.x > 0.0 && rect.size.y > 0.0
}

fn preview_accessibility_excerpt(content: &str) -> String {
    const LIMIT: usize = 2_000;
    let mut chars = content.chars();
    let excerpt = chars.by_ref().take(LIMIT).collect::<String>();
    if chars.next().is_some() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

pub fn accessibility_tree(snapshot: &WorkspaceSnapshot, physical_scale: f64) -> TreeUpdate {
    let scale = if physical_scale.is_finite() && physical_scale > 0.0 {
        physical_scale
    } else {
        1.0
    };
    let root_id = NodeId(0);
    let child_ids = snapshot
        .nodes
        .iter()
        .map(|node| NodeId(node.id.0))
        .collect::<Vec<_>>();
    let mut root = Node::new(Role::Window);
    root.set_label("Zode");
    root.set_bounds(physical_rect(snapshot.layout.viewport, scale));
    root.set_children(child_ids);

    let mut nodes = Vec::with_capacity(snapshot.nodes.len() + 1);
    nodes.push((root_id, root));
    for source in &snapshot.nodes {
        let mut target = Node::new(source.role);
        target.set_label(source.name.clone());
        if let Some(value) = source.value.as_ref() {
            target.set_value(value.clone());
        }
        target.set_bounds(physical_rect(source.rect, scale));
        for action in source.actions.iter().copied() {
            target.add_action(action);
        }
        if let Some(toggled) = source.toggled {
            target.set_toggled(toggled);
        }
        if source.disabled {
            target.set_disabled();
        }
        nodes.push((NodeId(source.id.0), target));
    }

    let focus = snapshot
        .focused
        .filter(|focused| snapshot.node(*focused).is_some())
        .map_or(root_id, |focused| NodeId(focused.0));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(root_id)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

#[allow(clippy::too_many_arguments)]
fn node(
    id: WidgetId,
    rect: Rect,
    role: Role,
    name: &str,
    value: Option<String>,
    actions: Vec<Action>,
    focus_order: Option<u32>,
    cursor: CursorHint,
) -> InteractionNode {
    InteractionNode {
        id,
        rect,
        role,
        name: name.into(),
        value,
        actions,
        focus_order,
        cursor,
        toggled: None,
        disabled: false,
    }
}

fn next_order(order: &mut u32) -> Option<u32> {
    let current = *order;
    *order = (*order).saturating_add(1);
    Some(current)
}

fn physical_rect(rect: Rect, scale: f64) -> accesskit::Rect {
    accesskit::Rect {
        x0: f64::from(rect.origin.x) * scale,
        y0: f64::from(rect.origin.y) * scale,
        x1: f64::from(rect.origin.x + rect.size.x) * scale,
        y1: f64::from(rect.origin.y + rect.size.y) * scale,
    }
}
