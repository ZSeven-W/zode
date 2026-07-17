use accesskit::Action;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    reduce_navigation_command, AppCommand, LoadState, NavigationOutcome, ShellRoute, ZodeAppState,
};
use zode_app_ui::{Key, KeyEvent, ProjectSidebar, WidgetId};

use super::DesktopApp;
use crate::presentation_bridge::PresentationQuery;

impl DesktopApp {
    /// Scrolls only the sidebar content beneath the fixed new-task row. A
    /// wheel gesture over the sidebar never leaks through to the transcript.
    pub(super) fn handle_sidebar_scroll_delta(&mut self, delta: f32) -> bool {
        let sidebar = self.frame_snapshot.layout.sidebar;
        if !sidebar_accepts_scroll(
            self.app_state.presentation.route,
            self.app_state.shell.sidebar_open,
            ProjectSidebar::scroll_viewport(sidebar),
            self.window_state.cursor_logical,
        ) {
            return false;
        }
        if let Some(command) = ProjectSidebar::scroll_command(sidebar, &self.app_state, delta) {
            if reduce_navigation_command(&mut self.app_state, command) == NavigationOutcome::Applied
            {
                self.invalidate_frame_snapshot_for_scroll();
                self.request_redraw();
            }
        }
        true
    }

    pub(super) fn handle_sidebar_shortcut(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed || !event.modifiers.primary() || self.app_state.project_picker.open {
            return false;
        }
        if is_new_task_shortcut(event) {
            self.enqueue_command(AppCommand::BeginTask {
                workspace_uri: self.app_state.active_available_workspace().cloned(),
            });
            return true;
        }
        if is_toggle_primary_sidebar_shortcut(event)
            && !matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
        {
            self.enqueue_command(AppCommand::TogglePrimarySidebar);
            return true;
        }
        if matches!(self.app_state.presentation.route, ShellRoute::Settings(_)) {
            return false;
        }
        let Some(number) = shortcut_number(event) else {
            return false;
        };
        let Some(session) = ProjectSidebar::shortcut_session(&self.app_state, number) else {
            return false;
        };
        self.enqueue_command(AppCommand::SelectSession(session));
        true
    }

    pub(super) fn handle_sidebar_accessibility_scroll(&mut self, action: Action) -> bool {
        let viewport = ProjectSidebar::scroll_viewport(self.frame_snapshot.layout.sidebar);
        let delta = match action {
            Action::ScrollUp => -viewport.size.y * 0.8,
            Action::ScrollDown => viewport.size.y * 0.8,
            _ => return false,
        };
        if let Some(command) = ProjectSidebar::scroll_command(
            self.frame_snapshot.layout.sidebar,
            &self.app_state,
            delta,
        ) {
            if reduce_navigation_command(&mut self.app_state, command) == NavigationOutcome::Applied
            {
                self.invalidate_frame_snapshot_for_scroll();
                self.request_redraw();
            }
        }
        true
    }

    /// Lazily resolves branch metadata for the session preview card. Paint
    /// stays side-effect free; pointer movement requests the context once and
    /// the presentation bridge wakes the window when it is ready.
    pub(super) fn request_sidebar_hover_context(&mut self, hovered: Option<WidgetId>) {
        let Some(query) = sidebar_hover_environment_query(
            &self.app_state,
            self.frame_snapshot.layout.sidebar,
            hovered,
        ) else {
            return;
        };
        if let Some(bridge) = self.presentation_queries.as_mut() {
            let _ = bridge.request(&mut self.app_state, query);
        }
    }
}

fn sidebar_hover_environment_query(
    state: &ZodeAppState,
    sidebar: Rect,
    hovered: Option<WidgetId>,
) -> Option<PresentationQuery> {
    let hovered = hovered?;
    let row = ProjectSidebar::layout(sidebar, state)
        .rows
        .into_iter()
        .find(|row| {
            row.session().is_some()
                && (row.id == hovered
                    || row.pin_id == Some(hovered)
                    || row.archive_id == Some(hovered))
        })?;
    let session = row.session()?.clone();
    if session.session_id.starts_with("local-error-") {
        return None;
    }
    let workspace_uri = row.workspace_uri?;
    let context = state
        .presentation
        .sessions
        .get(&session)
        .map(|presentation| &presentation.context);
    if matches!(context, Some(LoadState::Loading))
        || context
            .and_then(LoadState::ready)
            .is_some_and(|context| context.workspace_uri == workspace_uri)
    {
        return None;
    }
    Some(PresentationQuery::Environment {
        session,
        workspace_uri,
    })
}

fn sidebar_accepts_scroll(
    route: ShellRoute,
    sidebar_open: bool,
    viewport: Rect,
    cursor: Point2D,
) -> bool {
    sidebar_open && !matches!(route, ShellRoute::Settings(_)) && viewport.contains(cursor)
}

fn shortcut_number(event: &KeyEvent) -> Option<usize> {
    let Key::Character(value) = &event.key else {
        return None;
    };
    value
        .chars()
        .next()
        .filter(|_| value.chars().count() == 1)
        .and_then(|value| value.to_digit(10))
        .map(|value| value as usize)
        .filter(|value| (1..=5).contains(value))
}

fn is_new_task_shortcut(event: &KeyEvent) -> bool {
    event.pressed
        && event.modifiers.primary()
        && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("n"))
}

fn is_toggle_primary_sidebar_shortcut(event: &KeyEvent) -> bool {
    event.pressed
        && event.modifiers.primary()
        && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("b"))
}

#[cfg(test)]
mod tests {
    use jian_widgets::{Point2D, Rect};
    use zode_app_model::{
        EnvironmentSnapshot, LoadState, ProjectState, SettingsCategory, ShellRoute,
    };
    use zode_app_ui::{Key, KeyEvent, Modifiers, ProjectSidebar};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::{
        is_new_task_shortcut, is_toggle_primary_sidebar_shortcut, shortcut_number,
        sidebar_accepts_scroll, sidebar_hover_environment_query,
    };
    use crate::presentation_bridge::PresentationQuery;

    #[test]
    fn sidebar_scroll_capture_is_limited_to_its_visible_middle() {
        let viewport = Rect::xywh(0.0, 120.0, 240.0, 600.0);
        assert!(sidebar_accepts_scroll(
            ShellRoute::Conversation,
            true,
            viewport,
            Point2D::new(120.0, 300.0),
        ));
        assert!(!sidebar_accepts_scroll(
            ShellRoute::Conversation,
            true,
            viewport,
            Point2D::new(400.0, 300.0),
        ));
        assert!(!sidebar_accepts_scroll(
            ShellRoute::Settings(SettingsCategory::General),
            true,
            viewport,
            Point2D::new(120.0, 300.0),
        ));
    }

    #[test]
    fn only_single_digit_sidebar_shortcuts_one_through_five_are_resolved() {
        let event = |value: &str| KeyEvent {
            key: Key::Character(value.into()),
            modifiers: Modifiers::SUPER,
            pressed: true,
        };
        assert_eq!(shortcut_number(&event("1")), Some(1));
        assert_eq!(shortcut_number(&event("5")), Some(5));
        assert_eq!(shortcut_number(&event("0")), None);
        assert_eq!(shortcut_number(&event("12")), None);
    }

    #[test]
    fn command_n_is_the_global_new_task_shortcut() {
        let event = |value: &str, modifiers: Modifiers, pressed: bool| KeyEvent {
            key: Key::Character(value.into()),
            modifiers,
            pressed,
        };

        assert!(is_new_task_shortcut(&event("n", Modifiers::SUPER, true)));
        assert!(is_new_task_shortcut(&event("N", Modifiers::SUPER, true)));
        assert!(!is_new_task_shortcut(&event("n", Modifiers::NONE, true)));
        assert!(!is_new_task_shortcut(&event("n", Modifiers::SUPER, false)));
    }

    #[test]
    fn command_b_is_the_primary_sidebar_toggle_shortcut() {
        let event = |value: &str, modifiers: Modifiers, pressed: bool| KeyEvent {
            key: Key::Character(value.into()),
            modifiers,
            pressed,
        };

        assert!(is_toggle_primary_sidebar_shortcut(&event(
            "b",
            Modifiers::SUPER,
            true
        )));
        assert!(is_toggle_primary_sidebar_shortcut(&event(
            "B",
            Modifiers::SUPER,
            true
        )));
        assert!(!is_toggle_primary_sidebar_shortcut(&event(
            "b",
            Modifiers::NONE,
            true
        )));
        assert!(!is_toggle_primary_sidebar_shortcut(&event(
            "b",
            Modifiers::SUPER,
            false
        )));
    }

    #[test]
    fn session_hover_requests_missing_branch_context_once() {
        let mut state = zode_app_model::demo_state();
        let workspace = WorkspaceUri::new("file:///repo/openpencil").unwrap();
        let session = SessionLocator::new(state.host.node_id, "hover-context");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace.clone(),
            title: "Hover context".into(),
            updated_at_ms: 1,
            status: ThreadStatus::Idle,
        });
        let sidebar = Rect::xywh(0.0, 0.0, 240.0, 800.0);
        let row = ProjectSidebar::dynamic_row_layout(sidebar, &state)
            .into_iter()
            .find(|row| row.session() == Some(&session))
            .unwrap();

        assert_eq!(
            sidebar_hover_environment_query(&state, sidebar, row.pin_id),
            Some(PresentationQuery::Environment {
                session: session.clone(),
                workspace_uri: workspace.clone(),
            })
        );

        state
            .presentation
            .sessions
            .entry(session)
            .or_default()
            .context = LoadState::Ready(EnvironmentSnapshot {
            workspace_uri: workspace,
            branch: Some("v0.8.1".into()),
            subagents: Vec::new(),
            background_processes: Vec::new(),
            sources: Vec::new(),
        });
        assert_eq!(
            sidebar_hover_environment_query(&state, sidebar, row.pin_id),
            None
        );
    }
}
