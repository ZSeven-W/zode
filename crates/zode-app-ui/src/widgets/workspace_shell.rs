use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::{
    BranchCatalogState, ComposerContextMenu, ConnectionState, ProjectPickerAnchor, SecondaryPane,
    ShellRoute, TaskLaunchMode, ZodeAppState,
};

use super::project_sidebar::workspace_label;
use super::{
    ComingSoonPage, Composer, ComposerContextMenu as ComposerContextMenuWidget,
    ComposerFooterMenuWidget, DocumentPreview, EmptyState, EnvironmentPanel, IntegrationsPage,
    ProjectPicker, ProjectPickerViewState, ProjectSidebar, ReviewPanel, SettingsPanel,
    TerminalGrid, TerminalPanel, TerminalSecondaryPanel, TerminalSelection, ThreadHeader,
    ThreadTranscript, UnavailableSecondaryPanel, WindowChrome,
};
use crate::TRANSCRIPT_COMPOSER_GAP;
use crate::{
    Insets, PinnedSummaryMode, RectExt, WidgetId, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme,
    COMPOSER_BRANCH_ID, COMPOSER_LOCATION_ID, COMPOSER_PROJECT_ID,
};

/// Paints the complete platform-neutral workbench shell in stable z-order.
pub struct WorkspaceShell;

impl WorkspaceShell {
    /// Paint entry point for the immutable snapshot shared with hit testing and a11y.
    pub fn paint_snapshot(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let input = TextInputState::with_text(state.composer.draft.clone());
        let project_picker = ProjectPickerViewState {
            open: state.project_picker.open,
            query: state.project_picker.search.clone(),
        };
        let project_search = TextInputState::with_text(project_picker.query.clone());
        Self::paint_snapshot_content(
            painter,
            snapshot,
            state,
            &input,
            None,
            None,
            None,
            false,
            Some((&project_picker, &project_search)),
            None,
            None,
            theme,
        )
    }

    pub fn paint(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot(painter, &snapshot, state, theme)
    }

    pub fn paint_with_composer_input(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_content(
            painter,
            &snapshot,
            state,
            composer_input,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            theme,
        )
    }

    pub fn paint_with_terminal(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let input = TextInputState::with_text(state.composer.draft.clone());
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_content(
            painter,
            &snapshot,
            state,
            &input,
            Some(terminal_grid),
            terminal_selection,
            None,
            false,
            None,
            None,
            None,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_with_composer_and_terminal_input(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_with_composer_and_terminal_input(
            painter,
            &snapshot,
            state,
            composer_input,
            terminal_grid,
            terminal_selection,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_snapshot_with_composer_and_terminal_input(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        Self::paint_snapshot_content(
            painter,
            snapshot,
            state,
            composer_input,
            Some(terminal_grid),
            terminal_selection,
            None,
            false,
            None,
            None,
            None,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_snapshot_with_hovered_widget(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        Self::paint_snapshot_content(
            painter,
            snapshot,
            state,
            composer_input,
            Some(terminal_grid),
            terminal_selection,
            hovered,
            false,
            None,
            None,
            None,
            theme,
        )
    }

    /// Paints the normal workspace plus the transient project picker overlay.
    /// The host owns both editable buffers and supplies the exact input state
    /// used for caret, selection and IME painting.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_snapshot_with_project_picker(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        project_picker: &ProjectPickerViewState,
        project_search_input: &TextInputState,
        branch_search_input: &TextInputState,
        session_rename_input: &TextInputState,
        hovered: Option<WidgetId>,
        show_sidebar_shortcuts: bool,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        Self::paint_snapshot_content(
            painter,
            snapshot,
            state,
            composer_input,
            Some(terminal_grid),
            terminal_selection,
            hovered,
            show_sidebar_shortcuts,
            Some((project_picker, project_search_input)),
            Some(branch_search_input),
            Some(session_rename_input),
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_snapshot_content(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: Option<&TerminalGrid>,
        terminal_selection: Option<TerminalSelection>,
        hovered: Option<WidgetId>,
        show_sidebar_shortcuts: bool,
        project_picker: Option<(&ProjectPickerViewState, &TextInputState)>,
        branch_search_input: Option<&TextInputState>,
        session_rename_input: Option<&TextInputState>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = snapshot.clone();
        let geometry = snapshot.layout;
        painter.begin_frame();
        WindowChrome::paint(painter, geometry.viewport, &geometry, theme);
        if !matches!(state.presentation.route, ShellRoute::Settings(_)) {
            ProjectSidebar::paint_with_interaction(
                painter,
                geometry.sidebar,
                state,
                snapshot.focused,
                hovered,
                show_sidebar_shortcuts,
                theme,
            );
        }
        match state.presentation.route {
            ShellRoute::Conversation => {
                ThreadHeader::paint_with_pinned_summary(
                    painter,
                    geometry.top_bar,
                    state,
                    geometry.pinned_summary,
                    theme,
                );
            }
            ShellRoute::Terminal => {
                ThreadHeader::paint_title_only(painter, geometry.top_bar, state, theme);
            }
            ShellRoute::Settings(_) | ShellRoute::Integrations(_) | ShellRoute::ComingSoon(_) => {}
        }

        let split_fallback = state.presentation.route == ShellRoute::Conversation
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
            && geometry.review_panel.size.x <= 0.0;
        match state.presentation.route {
            ShellRoute::Settings(_) => {
                let workspace = SettingsPanel::active_workspace_uri(state);
                SettingsPanel::paint_page(painter, &snapshot, state, workspace, theme);
            }
            ShellRoute::Integrations(_) => {
                IntegrationsPage::paint_with_focus(
                    painter,
                    geometry.primary_surface,
                    state,
                    snapshot.focused,
                    theme,
                );
            }
            ShellRoute::ComingSoon(feature) => {
                ComingSoonPage::paint(painter, geometry.primary_surface, feature, theme);
            }
            ShellRoute::Terminal => paint_terminal(
                painter,
                geometry,
                state,
                terminal_grid,
                terminal_selection,
                theme,
            ),
            ShellRoute::Conversation if split_fallback => match state.presentation.secondary_pane {
                Some(SecondaryPane::Review) => {
                    ReviewPanel::paint_state(painter, geometry.primary_surface, state, theme)
                }
                Some(SecondaryPane::DocumentPreview) => {
                    DocumentPreview::paint(painter, geometry.primary_surface, state, theme)
                }
                Some(SecondaryPane::Terminal) => paint_terminal_secondary(
                    painter,
                    geometry.primary_surface,
                    state,
                    terminal_grid,
                    terminal_selection,
                    theme,
                ),
                Some(
                    pane
                    @ (SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask),
                ) => {
                    UnavailableSecondaryPanel::paint(painter, geometry.primary_surface, pane, theme)
                }
                Some(SecondaryPane::Environment) | None => {}
            },
            ShellRoute::Conversation => {
                let fallback_branch_search =
                    TextInputState::with_text(state.composer.branch_picker.query.clone());
                paint_conversation(
                    painter,
                    &snapshot,
                    state,
                    composer_input,
                    hovered,
                    project_picker,
                    branch_search_input.unwrap_or(&fallback_branch_search),
                    theme,
                )
            }
        }

        if state.presentation.route == ShellRoute::Conversation {
            if geometry.pinned_summary != PinnedSummaryMode::Hidden
                && geometry.context_panel.size.x > 0.0
            {
                EnvironmentPanel::paint(painter, geometry.context_panel, state, theme);
            }
            match state.presentation.secondary_pane {
                Some(SecondaryPane::Environment) => {}
                Some(SecondaryPane::Review) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    ReviewPanel::paint_state(painter, geometry.review_panel, state, theme);
                }
                Some(SecondaryPane::DocumentPreview) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    DocumentPreview::paint(painter, geometry.review_panel, state, theme);
                }
                Some(SecondaryPane::Terminal) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    paint_terminal_secondary(
                        painter,
                        geometry.review_panel,
                        state,
                        terminal_grid,
                        terminal_selection,
                        theme,
                    );
                }
                Some(
                    pane
                    @ (SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask),
                ) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    UnavailableSecondaryPanel::paint(painter, geometry.review_panel, pane, theme);
                }
                Some(
                    SecondaryPane::Review
                    | SecondaryPane::DocumentPreview
                    | SecondaryPane::Terminal
                    | SecondaryPane::Browser
                    | SecondaryPane::Files
                    | SecondaryPane::SideTask,
                )
                | None => {}
            }
        }
        if !matches!(state.presentation.route, ShellRoute::Settings(_)) {
            ProjectSidebar::paint_hover_overlay(
                painter,
                geometry.sidebar,
                state,
                snapshot.focused,
                hovered,
                theme,
            );
        }
        if state.presentation.route == ShellRoute::Conversation {
            if let Some(rename_input) = session_rename_input {
                ThreadHeader::paint_overlays_with_rename_input(
                    painter,
                    geometry.top_bar,
                    geometry.viewport,
                    state,
                    rename_input,
                    snapshot.focused,
                    hovered,
                    theme,
                );
            } else {
                ThreadHeader::paint_overlays(
                    painter,
                    geometry.top_bar,
                    geometry.viewport,
                    state,
                    snapshot.focused,
                    hovered,
                    theme,
                );
            }
        }
        painter.end_frame();
        geometry
    }
}

fn paint_conversation(
    painter: &mut dyn Painter,
    snapshot: &WorkspaceSnapshot,
    state: &ZodeAppState,
    composer_input: &TextInputState,
    hovered: Option<WidgetId>,
    project_picker: Option<(&ProjectPickerViewState, &TextInputState)>,
    branch_search_input: &TextInputState,
    theme: &ZodeTheme,
) {
    let geometry = snapshot.layout;
    let workspace_label = current_workspace_label(state);
    let mut welcome_title = None;
    if state.current_session.is_none() {
        // Keep the reference empty-task composition anchored above the input
        // surface. Context/attachment strips may occupy the gap, but the
        // guidance itself never reaches that lower area.
        let input = Composer::layout_for_state(geometry.composer, state).input;
        let empty_bottom = (input.origin.y - TRANSCRIPT_COMPOSER_GAP)
            .max(geometry.transcript.origin.y)
            .min(geometry.primary_surface.max_y());
        let empty_rect = Rect::xywh(
            geometry.transcript.origin.x,
            geometry.transcript.origin.y,
            geometry.transcript.size.x,
            empty_bottom - geometry.transcript.origin.y,
        );
        welcome_title = EmptyState::welcome_title_layout(empty_rect, workspace_label.as_deref());
        EmptyState::paint_with_workspace_suggestions(
            painter,
            empty_rect,
            workspace_label.as_deref(),
            snapshot.focused == Some(crate::PROJECT_PICKER_TRIGGER_ID),
            hovered == Some(crate::PROJECT_PICKER_TRIGGER_ID),
            state.ui_preferences.task_suggestions,
            theme,
        );
    } else {
        ThreadTranscript::paint(painter, geometry.transcript, state, theme);
    }
    let branch = current_branch(state);
    let connection_label = composer_connection_label(state);
    let goal = current_goal_progress(state);
    let menu_active = if state.project_picker.open
        && state.project_picker.anchor == ProjectPickerAnchor::Composer
    {
        Some(COMPOSER_PROJECT_ID)
    } else {
        match state.composer.context_menu {
            Some(ComposerContextMenu::Location) => Some(COMPOSER_LOCATION_ID),
            Some(ComposerContextMenu::Branch) => Some(COMPOSER_BRANCH_ID),
            None => None,
        }
    };
    Composer::paint_input_with_workspace_app_context_interactions(
        painter,
        geometry.composer,
        composer_input,
        state,
        workspace_label.as_deref(),
        Some(connection_label),
        branch,
        goal,
        snapshot.focused,
        hovered,
        menu_active,
        theme,
    );
    let picker_trigger = match state.project_picker.anchor {
        ProjectPickerAnchor::Welcome => welcome_title.and_then(|title| title.project),
        ProjectPickerAnchor::Composer => Composer::context_interaction_layout(
            geometry.composer,
            state,
            workspace_label.as_deref(),
            Some(connection_label),
            branch,
        )
        .project
        .map(|chip| chip.rect),
    };
    if let (Some((picker, search_input)), Some(trigger)) = (project_picker, picker_trigger) {
        if let Some(layout) = ProjectPicker::layout(geometry.viewport, trigger, state, picker) {
            ProjectPicker::paint(
                painter,
                &layout,
                search_input,
                snapshot.focused,
                hovered,
                theme,
            );
        }
    }
    let context = Composer::context_interaction_layout(
        geometry.composer,
        state,
        workspace_label.as_deref(),
        Some(connection_label),
        branch,
    );
    if let Some(layout) = ComposerContextMenuWidget::layout(geometry.viewport, context, state) {
        ComposerContextMenuWidget::paint(
            painter,
            &layout,
            branch_search_input,
            snapshot.focused,
            hovered,
            theme,
        );
    }
    let input = Composer::layout_for_state(geometry.composer, state).input;
    if let Some(layout) = ComposerFooterMenuWidget::layout(geometry.viewport, input, state) {
        ComposerFooterMenuWidget::paint(painter, &layout, snapshot.focused, hovered, theme);
    }
}

fn current_goal_progress(state: &ZodeAppState) -> Option<&zode_app_model::GoalProgress> {
    let session = state.current_session.as_ref()?;
    let transcript = state.transcripts.get(session)?;
    if !transcript.busy {
        return None;
    }
    transcript.items.iter().rev().find_map(|item| match item {
        zode_app_model::TranscriptItem::GoalProgress(goal) => Some(goal),
        _ => None,
    })
}

/// Returns only a branch that was loaded for the workspace the composer targets.
///
/// A new-task composer has no current session yet, so it may reuse the most
/// recently loaded context for its active workspace. Keeping the workspace
/// identity check here prevents a branch from another project leaking into the
/// new task surface.
fn current_branch(state: &ZodeAppState) -> Option<&str> {
    if let Some(session) = state.current_session.as_ref() {
        let workspace = state.available_workspace_for_session(session)?;
        if state.is_projectless_workspace(workspace) {
            return None;
        }
        let context = state.presentation.sessions.get(session)?.context.ready()?;
        return verified_branch(context, workspace);
    }

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
        .filter(|thread| &thread.workspace_uri == workspace)
        .filter_map(|thread| {
            let context = state
                .presentation
                .sessions
                .get(&thread.session)?
                .context
                .ready()?;
            let branch = verified_branch(context, workspace)?;
            Some((thread.updated_at_ms, branch))
        })
        .max_by_key(|(updated_at_ms, _)| *updated_at_ms)
        .map(|(_, branch)| branch)
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

fn verified_branch<'a>(
    context: &'a zode_app_model::EnvironmentSnapshot,
    workspace: &zode_node_protocol::WorkspaceUri,
) -> Option<&'a str> {
    if &context.workspace_uri != workspace {
        return None;
    }
    context
        .branch
        .as_deref()
        .filter(|branch| !branch.trim().is_empty())
}

fn current_workspace_label(state: &ZodeAppState) -> Option<String> {
    let workspace = state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .or_else(|| state.active_available_workspace())?;
    if state.is_projectless_workspace(workspace) {
        return None;
    }
    Some(workspace_label(workspace, true))
}

fn paint_terminal(
    painter: &mut dyn Painter,
    geometry: WorkspaceLayout,
    state: &ZodeAppState,
    terminal_grid: Option<&TerminalGrid>,
    terminal_selection: Option<TerminalSelection>,
    theme: &ZodeTheme,
) {
    let fallback = TerminalGrid::new(1, 1);
    let grid = terminal_grid.unwrap_or(&fallback);
    let terminal_rect = Rect::xywh(
        geometry.transcript.origin.x,
        geometry.transcript.origin.y,
        geometry.transcript.size.x,
        geometry.composer.max_y() - geometry.transcript.origin.y,
    );
    TerminalPanel::paint(
        painter,
        terminal_rect,
        grid,
        &state.terminal,
        terminal_selection,
        theme,
    );
}

fn paint_terminal_secondary(
    painter: &mut dyn Painter,
    rect: Rect,
    state: &ZodeAppState,
    terminal_grid: Option<&TerminalGrid>,
    terminal_selection: Option<TerminalSelection>,
    theme: &ZodeTheme,
) {
    let fallback = TerminalGrid::new(1, 1);
    TerminalSecondaryPanel::paint(
        painter,
        rect,
        terminal_grid.unwrap_or(&fallback),
        &state.terminal,
        terminal_selection,
        theme,
    );
}
