use jian_widgets::Point2D;
use zode_app_model::{AppCommand, ZodeAppState};
use zode_app_ui::{ThreadHeader, WorkspaceSnapshot};

pub(super) fn close_session_menu_command(state: &ZodeAppState) -> Option<AppCommand> {
    let session = state.session_menu.as_ref()?;
    (state.current_session.as_ref() == Some(session)).then(|| AppCommand::ToggleSessionMenu {
        session: session.clone(),
    })
}

pub(super) fn session_menu_outside_click_command(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    position: Point2D,
) -> Option<AppCommand> {
    let command = close_session_menu_command(state)?;
    let inside = ThreadHeader::menu_layout(snapshot.layout.top_bar, state)
        .is_some_and(|menu| menu.rect.contains(position));
    (!inside).then_some(command)
}
