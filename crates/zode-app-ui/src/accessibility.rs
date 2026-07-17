use accesskit::{Action, Role, Toggled};
use jian_core::CursorHint;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{IntegrationsTab, PreviewState, SecondaryPane, ShellRoute, ZodeAppState};

use crate::{
    composer_queue_reserved_height, Composer, DocumentPreview, GlobalSearchViewState, Insets,
    PanelPicker, PinnedSummaryMode, ProjectPickerViewState, RectExt, ReviewPanel,
    SecondarySidebarLayoutOptions, SettingsPanel, SubagentsPanel, TerminalSecondaryPanel,
    ThreadTranscript, UnavailableSecondaryPanel, WorkspaceLayout, COMPOSER_ATTACHMENT_H,
    COMPOSER_H, COMPOSER_INPUT_H, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID, INTEGRATIONS_PLUGINS_TAB_ID,
    INTEGRATIONS_SKILLS_TAB_ID, SECONDARY_PANE_BREAKPOINT, SUBAGENTS_PANEL_CLOSE_ID,
    SUBAGENTS_PANEL_SHOW_MORE_ID, TERMINAL_SECONDARY_CLOSE_ID, UNAVAILABLE_SECONDARY_CLOSE_ID,
};

mod composer_footer;
mod empty_state;
mod environment;
mod global_search;
mod header;
mod helpers;
mod ids;
mod integrations;
mod project_picker;
mod queue;
mod settings;
mod sidebar;
mod transcript;
mod tree;

use composer_footer::{append_composer_footer_nodes, append_composer_footer_overlay};
use empty_state::append_empty_suggestion_nodes;
use environment::append_environment_nodes;
use global_search::append_global_search_overlay;
use header::{append_header_menu_nodes, append_header_nodes, append_open_with_menu_nodes};
use helpers::{coming_soon_focus, preview_accessibility_excerpt};
pub(crate) use ids::stable_widget_id;
use integrations::append_integration_nodes;
use project_picker::{
    append_composer_context_nodes, append_composer_context_overlay, append_picker_overlay,
    append_welcome_project_trigger,
};
use queue::{append_queue_menu_nodes, append_queue_nodes};
use settings::append_settings_nodes;
use sidebar::{append_collapsed_sidebar_nodes, append_sidebar_menu_nodes, append_sidebar_nodes};
use transcript::append_transcript_nodes;
pub use tree::accessibility_tree;

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
pub const INTEGRATIONS_ROOT_ID: WidgetId = WidgetId(193);
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
        let global_search = GlobalSearchViewState {
            open: state.global_search.open,
            query: state.global_search.query.clone(),
            active_index: state.global_search.active_index,
        };
        Self::build_internal(
            state,
            width,
            height,
            insets,
            Some(&picker),
            Some(&global_search),
            None,
        )
    }

    pub fn build_with_project_picker(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: &ProjectPickerViewState,
    ) -> Self {
        let global_search = GlobalSearchViewState {
            open: state.global_search.open,
            query: state.global_search.query.clone(),
            active_index: state.global_search.active_index,
        };
        Self::build_internal(
            state,
            width,
            height,
            insets,
            Some(project_picker),
            Some(&global_search),
            None,
        )
    }

    pub fn build_with_overlays(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: &ProjectPickerViewState,
        global_search: &GlobalSearchViewState,
    ) -> Self {
        Self::build_internal(
            state,
            width,
            height,
            insets,
            Some(project_picker),
            Some(global_search),
            None,
        )
    }

    /// Builds a snapshot with host-supplied sidebar geometry. This keeps
    /// paint, hit testing and accessibility on the same interpolated layout
    /// while a persistent sidebar transition is active.
    #[allow(clippy::too_many_arguments)]
    pub fn build_with_overlays_and_sidebar_options(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: &ProjectPickerViewState,
        global_search: &GlobalSearchViewState,
        sidebars: crate::SidebarLayoutOptions,
    ) -> Self {
        Self::build_internal(
            state,
            width,
            height,
            insets,
            Some(project_picker),
            Some(global_search),
            Some(sidebars),
        )
    }

    fn build_internal(
        state: &ZodeAppState,
        width: f32,
        height: f32,
        insets: Insets,
        project_picker: Option<&ProjectPickerViewState>,
        global_search: Option<&GlobalSearchViewState>,
        sidebar_options: Option<crate::SidebarLayoutOptions>,
    ) -> Self {
        let route = state.presentation.route;
        let queue_count = state
            .current_session
            .as_ref()
            .and_then(|session| state.message_queues.get(session))
            .map_or(0, |queue| queue.items.len());
        let show_composer_context_bar = state.composer_context_bar_visible();
        let composer_height =
            (if route == ShellRoute::Conversation && !show_composer_context_bar {
                COMPOSER_INPUT_H
            } else {
                COMPOSER_H
            }) + if route == ShellRoute::Conversation && !state.composer.attachments.is_empty() {
                COMPOSER_ATTACHMENT_H
            } else {
                0.0
            } + if route == ShellRoute::Conversation {
                composer_queue_reserved_height(queue_count)
            } else {
                0.0
            };
        let sidebars = sidebar_options.unwrap_or(crate::SidebarLayoutOptions {
            primary: crate::PrimarySidebarLayoutOptions {
                open: state.shell.sidebar_open,
                width: f32::from(state.ui_preferences.primary_sidebar_width),
                visibility: 1.0,
            },
            secondary: SecondarySidebarLayoutOptions {
                open: state.presentation.secondary_sidebar_open,
                width: f32::from(state.ui_preferences.secondary_sidebar_width),
                pinned_summary_overlay: state.presentation.pinned_summary_overlay_open,
                visibility: 1.0,
                pinned_summary_visibility: 1.0,
            },
        });
        let animated_summary_present =
            sidebar_options.is_some() && sidebars.secondary.pinned_summary_visibility > 0.0;
        let auto_pinned_summary = route == ShellRoute::Conversation
            && state.current_session.is_some()
            && !state.presentation.pinned_summary_auto_hidden
            && !state.presentation.secondary_sidebar_open
            && width >= SECONDARY_PANE_BREAKPOINT;
        let explicit_pinned_summary = if sidebar_options.is_some() {
            sidebars.secondary.pinned_summary_overlay && animated_summary_present
        } else {
            state.presentation.pinned_summary_overlay_open
        };
        let layout_secondary =
            if auto_pinned_summary || explicit_pinned_summary || animated_summary_present {
                Some(SecondaryPane::Environment)
            } else {
                None
            };
        let layout = WorkspaceLayout::compute_presentation_with_sidebar_options(
            width,
            height,
            insets,
            route,
            layout_secondary,
            composer_height,
            sidebars,
        );
        let mut nodes = Vec::new();
        let mut focus_order = 0;
        let split_fallback = route == ShellRoute::Conversation
            && state.presentation.secondary_sidebar_open
            && !visible_rect(layout.review_panel)
            && !visible_rect(layout.secondary_sidebar_content)
            && visible_rect(layout.primary_surface);

        if !matches!(route, ShellRoute::Settings(_)) {
            if state.shell.sidebar_open && visible_rect(layout.sidebar) {
                append_sidebar_nodes(&mut nodes, &layout, &mut focus_order, state);
            } else if !state.shell.sidebar_open && visible_rect(layout.top_bar) {
                append_collapsed_sidebar_nodes(&mut nodes, &layout, &mut focus_order);
            }
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
                        // Footer controls live inside the text-input surface. Append their
                        // interaction nodes after the input so reverse-order hit testing
                        // reaches the visible controls instead of the enclosing editor.
                        append_composer_footer_nodes(&mut nodes, &layout, &mut focus_order, state);
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
        append_secondary_nodes(
            &mut nodes,
            &layout,
            &mut focus_order,
            state,
            sidebars.secondary.open,
        );
        if visible_rect(layout.sidebar) && !matches!(route, ShellRoute::Settings(_)) {
            append_sidebar_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
        }
        let header_overlay_focus = if route == ShellRoute::Conversation {
            let focus = append_header_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
            let open_with_focus =
                append_open_with_menu_nodes(&mut nodes, &layout, &mut focus_order, state);
            open_with_focus.or(focus)
        } else {
            None
        };
        let mut focused = if split_fallback {
            match state.presentation.secondary_pane {
                Some(SecondaryPane::DocumentPreview) => Some(DOCUMENT_PREVIEW_CLOSE_ID),
                Some(SecondaryPane::Terminal) => Some(TERMINAL_ID),
                Some(SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask) => {
                    Some(UNAVAILABLE_SECONDARY_CLOSE_ID)
                }
                Some(SecondaryPane::Subagents) => Some(SUBAGENTS_PANEL_CLOSE_ID),
                Some(SecondaryPane::Review) => Some(REVIEW_CLOSE_ID),
                Some(SecondaryPane::Environment) | None => None,
            }
            .and_then(|id| {
                nodes
                    .iter()
                    .find(|node| node.id == id && node.focus_order.is_some())
                    .map(|node| node.id)
            })
            .or_else(|| {
                nodes
                    .iter()
                    .find(|node| {
                        matches!(
                            node.id,
                            crate::SECONDARY_HOME_REVIEW_ID
                                | crate::SECONDARY_HOME_TERMINAL_ID
                                | crate::SECONDARY_HOME_BROWSER_ID
                                | crate::SECONDARY_HOME_FILES_ID
                                | crate::SECONDARY_HOME_SIDE_TASK_ID
                        ) && node.focus_order.is_some()
                    })
                    .map(|node| node.id)
            })
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
        if let Some(global_search) = global_search {
            if let Some(search_focus) = append_global_search_overlay(
                &mut nodes,
                &layout,
                &mut focus_order,
                state,
                global_search,
            ) {
                focused = Some(search_focus);
            }
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
        let nodes = self
            .nodes
            .iter()
            .position(|node| node.id == crate::GLOBAL_SEARCH_SURFACE_ID)
            .map_or(self.nodes.as_slice(), |overlay_start| {
                &self.nodes[overlay_start..]
            });
        let mut ordered = nodes
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
    secondary_sidebar_open: bool,
) {
    if state.presentation.route != ShellRoute::Conversation {
        return;
    }
    let has_pinned_summary =
        layout.pinned_summary != PinnedSummaryMode::Hidden && visible_rect(layout.context_panel);
    if has_pinned_summary && layout.pinned_summary != PinnedSummaryMode::Overlay {
        append_environment_nodes(nodes, layout, focus_order, state);
    }
    if !secondary_sidebar_open {
        if has_pinned_summary && layout.pinned_summary == PinnedSummaryMode::Overlay {
            append_environment_nodes(nodes, layout, focus_order, state);
        }
        return;
    }
    let split_visible = visible_rect(layout.review_panel);
    if !split_visible && !state.presentation.secondary_sidebar_open {
        return;
    }
    let panel_rect = if split_visible {
        layout.secondary_sidebar_content
    } else {
        layout.primary_surface
    };
    let panel_clip = if split_visible {
        layout.review_panel
    } else {
        layout.primary_surface
    };
    let panel_nodes_start = nodes.len();
    match state.presentation.secondary_pane {
        Some(SecondaryPane::Review) => {
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
        Some(SecondaryPane::Subagents) => {
            let panel_layout = SubagentsPanel::layout(panel_rect, state);
            if visible_rect(panel_layout.close_button) {
                nodes.push(node(
                    SUBAGENTS_PANEL_CLOSE_ID,
                    panel_layout.close_button,
                    Role::Button,
                    "关闭子智能体面板",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
            nodes.push(node(
                WidgetId(262),
                panel_rect,
                Role::Group,
                "子智能体",
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            ));
            let now = SubagentsPanel::now_ms();
            let session = state.current_session.clone();
            for row in panel_layout
                .running_rows
                .iter()
                .chain(panel_layout.completed_rows.iter())
            {
                let Some(rect) = ThreadTranscript::clip_to_viewport(row.rect, panel_rect) else {
                    continue;
                };
                let id = session
                    .as_ref()
                    .map(|session| stable_widget_id(0x67, &(session, row.subagent.id.as_str())))
                    .unwrap_or(WidgetId(0));
                nodes.push(node(
                    id,
                    rect,
                    Role::Group,
                    &SubagentsPanel::row_label(&row.subagent, now),
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
            }
            if let Some(show_more) = panel_layout.show_more.filter(|rect| visible_rect(*rect)) {
                nodes.push(node(
                    SUBAGENTS_PANEL_SHOW_MORE_ID,
                    show_more,
                    Role::Button,
                    &panel_layout.show_more_label,
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                ));
            }
        }
        Some(pane @ (SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask)) => {
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
        Some(SecondaryPane::Environment) | None => {
            if let Some(home) = PanelPicker::home_layout(panel_rect, state) {
                for item in home.items {
                    let mut interaction = node(
                        item.id,
                        item.rect,
                        Role::Button,
                        item.label,
                        item.unavailable_reason
                            .map(str::to_owned)
                            .or_else(|| item.shortcut.map(str::to_owned)),
                        if item.enabled {
                            vec![Action::Click, Action::Focus]
                        } else {
                            Vec::new()
                        },
                        item.enabled.then(|| next_order(focus_order)).flatten(),
                        if item.enabled {
                            CursorHint::Pointer
                        } else {
                            CursorHint::Default
                        },
                    );
                    interaction.disabled = !item.enabled;
                    nodes.push(interaction);
                }
            }
        }
    }
    clip_nodes_since(nodes, panel_nodes_start, panel_clip);
    if has_pinned_summary && layout.pinned_summary == PinnedSummaryMode::Overlay {
        append_environment_nodes(nodes, layout, focus_order, state);
    }
}

fn clip_nodes_since(nodes: &mut Vec<InteractionNode>, start: usize, clip: Rect) {
    let tail = nodes.split_off(start);
    nodes.extend(tail.into_iter().filter_map(|mut interaction| {
        interaction.rect = ThreadTranscript::clip_to_viewport(interaction.rect, clip)?;
        Some(interaction)
    }));
}

fn visible_rect(rect: Rect) -> bool {
    rect.size.x > 0.0 && rect.size.y > 0.0
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
