use jian_widgets::Point2D;
use zode_app_model::{AppCommand, ZodeAppState};
use zode_app_ui::{Key, KeyEvent, Modifiers, ProjectSidebar, WorkspaceSnapshot};

use super::DesktopApp;

#[derive(Debug, Clone, PartialEq)]
enum SidebarMenuPointerOutcome {
    Ignored,
    Captured,
    Actionable,
    Close(AppCommand),
}

pub(super) fn sidebar_menu_close_command(state: &ZodeAppState) -> Option<AppCommand> {
    if let Some(workspace_uri) = state.sidebar.project_menu.as_ref() {
        return Some(AppCommand::ToggleProjectMenu {
            workspace_uri: workspace_uri.clone(),
        });
    }
    state
        .sidebar
        .section_menu
        .map(AppCommand::ToggleSidebarSectionMenu)
}

fn sidebar_menu_pointer_outcome(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    position: Point2D,
) -> SidebarMenuPointerOutcome {
    let Some(menu) = ProjectSidebar::menu_layout(snapshot.layout.sidebar, state) else {
        return SidebarMenuPointerOutcome::Ignored;
    };
    if !menu.rect.contains(position) {
        return sidebar_menu_close_command(state)
            .map(SidebarMenuPointerOutcome::Close)
            .unwrap_or(SidebarMenuPointerOutcome::Ignored);
    }
    let actionable = snapshot
        .hit_test(position)
        .is_some_and(|id| ProjectSidebar::command_for_widget(state, id).is_some());
    if actionable {
        SidebarMenuPointerOutcome::Actionable
    } else {
        SidebarMenuPointerOutcome::Captured
    }
}

impl DesktopApp {
    pub(super) fn handle_sidebar_menu_pointer(&mut self, position: Point2D) -> bool {
        match sidebar_menu_pointer_outcome(&self.app_state, &self.frame_snapshot, position) {
            SidebarMenuPointerOutcome::Ignored | SidebarMenuPointerOutcome::Actionable => false,
            SidebarMenuPointerOutcome::Captured => true,
            SidebarMenuPointerOutcome::Close(command) => {
                self.enqueue_command(command);
                true
            }
        }
    }

    pub(super) fn handle_sidebar_menu_key(&mut self, event: &KeyEvent) -> bool {
        let Some(menu) =
            ProjectSidebar::menu_layout(self.frame_snapshot.layout.sidebar, &self.app_state)
        else {
            return false;
        };
        if !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            if let Some(command) = sidebar_menu_close_command(&self.app_state) {
                self.enqueue_command(command);
            }
            return true;
        }
        if event.key == Key::Tab || matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let ids = menu
                .items
                .iter()
                .filter(|item| item.enabled)
                .map(|item| item.id)
                .collect::<Vec<_>>();
            if ids.is_empty() {
                return true;
            }
            let backwards = event.key == Key::ArrowUp || event.modifiers.contains(Modifiers::SHIFT);
            let next = self
                .focused_widget
                .and_then(|focused| ids.iter().position(|id| *id == focused))
                .map(|index| {
                    if backwards {
                        (index + ids.len() - 1) % ids.len()
                    } else {
                        (index + 1) % ids.len()
                    }
                })
                .unwrap_or_else(|| if backwards { ids.len() - 1 } else { 0 });
            self.set_focused_widget(Some(ids[next]));
            return true;
        }
        !matches!(event.key, Key::Enter)
            && !matches!(&event.key, Key::Character(value) if value == " ")
    }
}

#[cfg(test)]
mod tests {
    use jian_widgets::Point2D;
    use zode_app_model::{AppCommand, ProjectState, SidebarSectionMenu};
    use zode_app_ui::{Insets, ProjectSidebar, RectExt, WorkspaceSnapshot};
    use zode_node_protocol::WorkspaceUri;

    use super::{
        sidebar_menu_close_command, sidebar_menu_pointer_outcome, SidebarMenuPointerOutcome,
    };

    #[test]
    fn escape_command_closes_either_sidebar_menu_kind() {
        let mut state = state_with_project_menu();
        let workspace = state.sidebar.project_menu.clone().unwrap();
        assert_eq!(
            sidebar_menu_close_command(&state),
            Some(AppCommand::ToggleProjectMenu {
                workspace_uri: workspace,
            })
        );
        state.sidebar.project_menu = None;
        state.sidebar.section_menu = Some(SidebarSectionMenu::Projects);
        assert_eq!(
            sidebar_menu_close_command(&state),
            Some(AppCommand::ToggleSidebarSectionMenu(
                SidebarSectionMenu::Projects
            ))
        );
    }

    #[test]
    fn menu_padding_is_captured_and_outside_click_closes() {
        let state = state_with_project_menu();
        let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
        let menu = ProjectSidebar::menu_layout(snapshot.layout.sidebar, &state).unwrap();
        let padding = Point2D::new(menu.rect.min_x() + 1.0, menu.rect.min_y() + 1.0);
        assert_eq!(
            sidebar_menu_pointer_outcome(&state, &snapshot, padding),
            SidebarMenuPointerOutcome::Captured
        );
        let first = menu.items[0].rect;
        assert_eq!(
            sidebar_menu_pointer_outcome(
                &state,
                &snapshot,
                Point2D::new(first.min_x() + 4.0, first.min_y() + 4.0),
            ),
            SidebarMenuPointerOutcome::Actionable
        );
        assert!(matches!(
            sidebar_menu_pointer_outcome(
                &state,
                &snapshot,
                Point2D::new(menu.rect.max_x() + 10.0, menu.rect.max_y() + 10.0),
            ),
            SidebarMenuPointerOutcome::Close(AppCommand::ToggleProjectMenu { .. })
        ));
    }

    fn state_with_project_menu() -> zode_app_model::ZodeAppState {
        let mut state = zode_app_model::demo_state();
        state.projects.clear();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.sidebar.project_menu = Some(workspace);
        state
    }
}
