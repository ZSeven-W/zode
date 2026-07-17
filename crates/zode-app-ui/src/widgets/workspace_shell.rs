use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::{ConnectionState, SecondaryPane, ShellRoute, ZodeAppState};

use super::{
    ComingSoonPage, Composer, EmptyState, EnvironmentPanel, IntegrationsPage, ProjectSidebar,
    ReviewPanel, SettingsPanel, TerminalGrid, TerminalPanel, TerminalSelection, ThreadHeader,
    ThreadTranscript, WindowChrome,
};
use crate::{Insets, RectExt, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme, COMPOSER_ID};

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
        Self::paint_snapshot_content(painter, snapshot, state, &input, None, None, theme)
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
        Self::paint_snapshot_content(painter, &snapshot, state, composer_input, None, None, theme)
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
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = snapshot.clone();
        let geometry = snapshot.layout;
        painter.begin_frame();
        WindowChrome::paint(painter, geometry.viewport, &geometry, theme);
        if !matches!(state.presentation.route, ShellRoute::Settings(_)) {
            ProjectSidebar::paint(painter, geometry.sidebar, state, theme);
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

        let review_fallback = state.presentation.route == ShellRoute::Conversation
            && state.presentation.secondary_pane == Some(SecondaryPane::Review)
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
            ShellRoute::Conversation if review_fallback => {
                ReviewPanel::paint_state(painter, geometry.primary_surface, state, theme);
            }
            ShellRoute::Conversation => {
                paint_conversation(painter, &snapshot, state, composer_input, theme)
            }
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
                Some(SecondaryPane::Environment | SecondaryPane::Review) | None => {}
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
    theme: &ZodeTheme,
) {
    let geometry = snapshot.layout;
    if state.current_session.is_none() {
        EmptyState::paint(painter, geometry.transcript, theme);
    } else {
        ThreadTranscript::paint(painter, geometry.transcript, state, theme);
    }
    let composer_rect = snapshot
        .node(COMPOSER_ID)
        .map(|node| node.rect)
        .filter(|rect| contained_by(*rect, geometry.primary_surface))
        .unwrap_or(geometry.composer);
    let branch = state
        .current_session_presentation()
        .and_then(|presentation| presentation.context.ready())
        .and_then(|context| context.branch.as_deref());
    let connection_label = match state.host.connection {
        ConnectionState::Local => "本地",
        ConnectionState::Connecting => "连接中",
        ConnectionState::Unavailable => "不可用",
    };
    Composer::paint_input_with_context(
        painter,
        composer_rect,
        composer_input,
        &state.composer,
        Some(connection_label),
        branch,
        theme,
    );
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

fn contained_by(inner: Rect, outer: Rect) -> bool {
    inner.min_x() >= outer.min_x()
        && inner.min_y() >= outer.min_y()
        && inner.max_x() <= outer.max_x()
        && inner.max_y() <= outer.max_y()
}
