use jian_widgets::Point2D;
use zode_app_model::{AppCommand, ZodeAppState};
use zode_app_ui::{PanelPicker, ThreadHeader, WorkspaceSnapshot};

use super::DesktopApp;

pub(super) fn close_panel_menu_command(state: &ZodeAppState) -> Option<AppCommand> {
    state
        .presentation
        .secondary_menu_open
        .then_some(AppCommand::CloseSecondaryMenu)
}

pub(super) fn panel_menu_outside_click_command(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    position: Point2D,
) -> Option<AppCommand> {
    let command = close_panel_menu_command(state)?;
    let anchor = ThreadHeader::layout(snapshot.layout.top_bar, state).panel_picker?;
    let inside = PanelPicker::menu_layout(anchor.rect, snapshot.layout.viewport, state)
        .is_some_and(|menu| menu.rect.contains(position));
    (!inside).then_some(command)
}

impl DesktopApp {
    pub(super) fn handle_panel_menu_pointer(&mut self, position: Point2D) -> bool {
        if let Some(command) =
            panel_menu_outside_click_command(&self.app_state, &self.frame_snapshot, position)
        {
            self.enqueue_command(command);
            return true;
        }
        if !self.app_state.presentation.secondary_menu_open {
            return false;
        }
        self.frame_snapshot
            .hit_test(position)
            .is_none_or(|id| ThreadHeader::command_for_widget(&self.app_state, id).is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::{close_panel_menu_command, panel_menu_outside_click_command};
    use zode_app_model::{demo_state, AppCommand};
    use zode_app_ui::{Insets, PanelPicker, ThreadHeader, WorkspaceSnapshot};

    #[test]
    fn escape_and_outside_click_close_but_inside_click_stays_open() {
        let mut state = demo_state();
        state.presentation.secondary_menu_open = true;
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 800.0, Insets::ZERO);
        let anchor = ThreadHeader::layout(snapshot.layout.top_bar, &state)
            .panel_picker
            .unwrap();
        let menu = PanelPicker::menu_layout(anchor.rect, snapshot.layout.viewport, &state).unwrap();

        assert_eq!(
            close_panel_menu_command(&state),
            Some(AppCommand::CloseSecondaryMenu)
        );
        assert_eq!(
            panel_menu_outside_click_command(
                &state,
                &snapshot,
                jian_widgets::Point2D::new(
                    menu.rect.origin.x + menu.rect.size.x / 2.0,
                    menu.rect.origin.y + menu.rect.size.y / 2.0,
                ),
            ),
            None
        );
        assert_eq!(
            panel_menu_outside_click_command(
                &state,
                &snapshot,
                jian_widgets::Point2D::new(20.0, 700.0),
            ),
            Some(AppCommand::CloseSecondaryMenu)
        );
    }
}
