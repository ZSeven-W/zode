use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::{ConnectionState, SecondaryPane, ShellRoute, ZodeAppState};

use super::project_sidebar::workspace_label;
use super::{
    ComingSoonPage, Composer, DocumentPreview, EmptyState, EnvironmentPanel, IntegrationsPage,
    ProjectPicker, ProjectPickerViewState, ProjectSidebar, ReviewPanel, SettingsPanel,
    TerminalGrid, TerminalPanel, TerminalSelection, ThreadHeader, ThreadTranscript, WindowChrome,
};
use crate::TRANSCRIPT_COMPOSER_GAP;
use crate::{Insets, RectExt, WidgetId, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme};

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
            Some((&project_picker, &project_search)),
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
            Some((project_picker, project_search_input)),
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
        project_picker: Option<(&ProjectPickerViewState, &TextInputState)>,
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
                theme,
            );
        }
        match state.presentation.route {
            ShellRoute::Conversation => {
                ThreadHeader::paint(painter, geometry.top_bar, state, theme);
            }
            ShellRoute::Terminal => {
                ThreadHeader::paint_title_only(painter, geometry.top_bar, state, theme);
            }
            ShellRoute::Settings(_) | ShellRoute::Integrations(_) | ShellRoute::ComingSoon(_) => {}
        }

        let split_fallback = state.presentation.route == ShellRoute::Conversation
            && matches!(
                state.presentation.secondary_pane,
                Some(SecondaryPane::Review | SecondaryPane::DocumentPreview)
            )
            && geometry.review_panel.size.x <= 0.0;
        match state.presentation.route {
            ShellRoute::Settings(_) => {
                let workspace = SettingsPanel::active_workspace_uri(state);
                SettingsPanel::paint_page(painter, &snapshot, state, workspace, theme);
            }
            ShellRoute::Integrations(_) => {
                IntegrationsPage::paint(painter, geometry.primary_surface, state, theme);
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
                Some(SecondaryPane::Environment) | None => {}
            },
            ShellRoute::Conversation => paint_conversation(
                painter,
                &snapshot,
                state,
                composer_input,
                hovered,
                project_picker,
                theme,
            ),
        }

        if state.presentation.route == ShellRoute::Conversation {
            match state.presentation.secondary_pane {
                Some(SecondaryPane::Environment) if geometry.context_panel.size.x > 0.0 => {
                    EnvironmentPanel::paint(painter, geometry.context_panel, state, theme);
                }
                Some(SecondaryPane::Review) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    ReviewPanel::paint_state(painter, geometry.review_panel, state, theme);
                }
                Some(SecondaryPane::DocumentPreview) if geometry.review_panel.size.x > 0.0 => {
                    painter.fill_rect(geometry.divider, theme.tokens.border);
                    DocumentPreview::paint(painter, geometry.review_panel, state, theme);
                }
                Some(
                    SecondaryPane::Environment
                    | SecondaryPane::Review
                    | SecondaryPane::DocumentPreview,
                )
                | None => {}
            }
        }
        if !matches!(state.presentation.route, ShellRoute::Settings(_)) {
            ProjectSidebar::paint_hover_overlay(painter, geometry.sidebar, state, hovered, theme);
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
        EmptyState::paint_with_workspace(
            painter,
            empty_rect,
            workspace_label.as_deref(),
            snapshot.focused == Some(crate::PROJECT_PICKER_TRIGGER_ID),
            hovered == Some(crate::PROJECT_PICKER_TRIGGER_ID),
            theme,
        );
    } else {
        ThreadTranscript::paint(painter, geometry.transcript, state, theme);
    }
    let branch = current_branch(state);
    let connection_label = match state.host.connection {
        ConnectionState::Local => "本地",
        ConnectionState::Connecting => "连接中",
        ConnectionState::Unavailable => "不可用",
    };
    let goal = current_goal_progress(state);
    Composer::paint_input_with_workspace_app_context(
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
        theme,
    );
    if let (Some((picker, search_input)), Some(trigger)) = (
        project_picker,
        welcome_title.and_then(|title| title.project),
    ) {
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
