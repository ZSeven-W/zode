use std::sync::Arc;

use jian_widgets::Point2D;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{AppCommand, ExternalApplication, ZodeAppState};
use zode_app_ui::{
    Key, KeyEvent, Modifiers, OpenWithMenu, ThreadHeader, WidgetId, WorkspaceSnapshot,
    OPEN_WITH_DROPDOWN_ID, OPEN_WITH_PRIMARY_ID,
};

use crate::{services::ExternalApplicationService, window_state::AppWake};

use super::DesktopApp;

pub(super) struct OpenWithEffect {
    service: Arc<dyn ExternalApplicationService>,
    results: mpsc::UnboundedReceiver<AppCommand>,
    result_sender: mpsc::UnboundedSender<AppCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
    in_flight: bool,
}

impl OpenWithEffect {
    pub(super) fn new(
        proxy: EventLoopProxy<AppWake>,
        service: Arc<dyn ExternalApplicationService>,
    ) -> Self {
        Self::with_wake(service, move || {
            let _ = proxy.send_event(AppWake::Redraw);
        })
    }

    fn with_wake(
        service: Arc<dyn ExternalApplicationService>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (result_sender, results) = mpsc::unbounded_channel();
        Self {
            service,
            results,
            result_sender,
            wake: Arc::new(wake),
            in_flight: false,
        }
    }

    pub(super) fn set_service(&mut self, service: Arc<dyn ExternalApplicationService>) {
        self.service = service;
        self.in_flight = false;
    }

    pub(super) fn request_catalog(&mut self) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        let service = Arc::clone(&self.service);
        let sender = self.result_sender.clone();
        let wake = Arc::clone(&self.wake);
        tokio::spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || service.installed_applications()).await;
            let command = match result {
                Ok(Ok(catalog)) => AppCommand::ExternalApplicationsLoaded(catalog),
                Ok(Err(error)) => AppCommand::ExternalApplicationsFailed(error.to_string()),
                Err(error) => AppCommand::ExternalApplicationsFailed(format!(
                    "application discovery worker failed: {error}"
                )),
            };
            if sender.send(command).is_ok() {
                wake();
            }
        });
    }

    pub(super) fn drain(&mut self) -> Vec<AppCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = self.results.try_recv() {
            self.in_flight = false;
            commands.push(command);
        }
        commands
    }

    fn request_open_workspace(
        &self,
        workspace: zode_node_protocol::WorkspaceUri,
        application: ExternalApplication,
    ) {
        let service = Arc::clone(&self.service);
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                service.open_workspace(&workspace, application)
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!(
                    "zode-app: opening workspace with {} failed: {error}",
                    application.label()
                ),
                Err(error) => eprintln!(
                    "zode-app: opening workspace worker for {} failed: {error}",
                    application.label()
                ),
            }
        });
    }
}

pub(super) fn open_with_escape_command(state: &ZodeAppState) -> Option<AppCommand> {
    state
        .open_with
        .menu_open
        .then_some(AppCommand::ToggleOpenWithMenu)
}

pub(super) fn open_with_outside_click_command(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    position: Point2D,
) -> Option<AppCommand> {
    let command = open_with_escape_command(state)?;
    let Some(anchor) = ThreadHeader::layout(snapshot.layout.top_bar, state).open_with else {
        return Some(command);
    };
    let inside = OpenWithMenu::menu_layout(anchor.rect, snapshot.layout.viewport, state)
        .is_some_and(|menu| menu.rect.contains(position));
    (!inside).then_some(command)
}

impl DesktopApp {
    pub(super) fn request_open_with_catalog(&mut self) {
        self.open_with_effect.request_catalog();
    }

    pub(super) fn drain_open_with_results(&mut self) -> usize {
        let commands = self.open_with_effect.drain();
        let count = commands.len();
        for command in commands {
            self.enqueue_command(command);
        }
        count
    }

    pub(super) fn apply_open_with_command(&self, command: &AppCommand) -> bool {
        consume_open_with_command(&self.app_state, &self.open_with_effect, command)
    }

    pub(super) fn handle_open_with_pointer(&mut self, position: Point2D) -> bool {
        if let Some(command) =
            open_with_outside_click_command(&self.app_state, &self.frame_snapshot, position)
        {
            self.enqueue_command(command);
            return true;
        }
        if !self.app_state.open_with.menu_open {
            return false;
        }
        self.frame_snapshot
            .hit_test(position)
            .is_none_or(|id| ThreadHeader::command_for_widget(&self.app_state, id).is_none())
    }

    pub(super) fn handle_open_with_key(&mut self, event: &KeyEvent) -> bool {
        if !self.app_state.open_with.menu_open {
            return false;
        }
        if !event.pressed {
            return true;
        }
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::ToggleOpenWithMenu);
            return true;
        }
        if event.key == Key::Tab || matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let mut ids = OpenWithMenu::focusable_ids(&self.app_state);
            if ids.is_empty() {
                ids.push(OPEN_WITH_DROPDOWN_ID);
            }
            let backwards = event.key == Key::ArrowUp || event.modifiers.contains(Modifiers::SHIFT);
            let current = self
                .focused_widget
                .and_then(|focused| ids.iter().position(|id| *id == focused));
            let index = match (backwards, current) {
                (false, Some(index)) => (index + 1) % ids.len(),
                (true, Some(0) | None) => ids.len() - 1,
                (true, Some(index)) => index - 1,
                (false, None) => 0,
            };
            self.set_focused_widget(Some(ids[index]));
            return true;
        }
        !matches!(event.key, Key::Enter)
            && !matches!(&event.key, Key::Character(value) if value == " ")
    }

    pub(super) fn sync_open_with_after_navigation(&self, command: &AppCommand) -> Option<WidgetId> {
        match command {
            AppCommand::ToggleOpenWithMenu => {
                if self.app_state.open_with.menu_open {
                    OpenWithMenu::focusable_ids(&self.app_state)
                        .into_iter()
                        .next()
                        .or(Some(OPEN_WITH_DROPDOWN_ID))
                } else {
                    Some(OPEN_WITH_DROPDOWN_ID)
                }
            }
            AppCommand::ExternalApplicationsLoaded(_)
            | AppCommand::ExternalApplicationsFailed(_) => {
                self.app_state.open_with.menu_open.then(|| {
                    OpenWithMenu::focusable_ids(&self.app_state)
                        .into_iter()
                        .next()
                        .unwrap_or(OPEN_WITH_DROPDOWN_ID)
                })
            }
            AppCommand::SelectExternalApplication { .. }
            | AppCommand::OpenWorkspaceExternally { .. } => Some(OPEN_WITH_PRIMARY_ID),
            _ => None,
        }
    }
}

fn consume_open_with_command(
    state: &ZodeAppState,
    effect: &OpenWithEffect,
    command: &AppCommand,
) -> bool {
    let AppCommand::OpenWorkspaceExternally {
        workspace_uri,
        application,
    } = command
    else {
        return false;
    };
    let current_workspace = state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .or_else(|| state.active_available_workspace())
        .filter(|workspace| !state.is_projectless_workspace(workspace));
    if current_workspace == Some(workspace_uri) {
        effect.request_open_workspace(workspace_uri.clone(), *application);
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        consume_open_with_command, open_with_escape_command, open_with_outside_click_command,
        OpenWithEffect,
    };
    use crate::services::{ExternalApplicationService, ServiceError};
    use zode_app_model::{
        demo_state, AppCommand, ExternalApplication, ExternalApplicationCatalog, LoadState,
        ProjectState,
    };
    use zode_app_ui::{Insets, OpenWithMenu, ThreadHeader, WorkspaceSnapshot};
    use zode_node_protocol::WorkspaceUri;

    #[derive(Default)]
    struct RecordingService(Mutex<Vec<(WorkspaceUri, ExternalApplication)>>);

    impl ExternalApplicationService for RecordingService {
        fn installed_applications(&self) -> Result<ExternalApplicationCatalog, ServiceError> {
            Ok(vec![ExternalApplication::Finder].into())
        }

        fn open_workspace(
            &self,
            workspace: &WorkspaceUri,
            application: ExternalApplication,
        ) -> Result<(), ServiceError> {
            self.0
                .lock()
                .unwrap()
                .push((workspace.clone(), application));
            Ok(())
        }
    }

    #[tokio::test]
    async fn only_the_current_local_workspace_is_opened() {
        let service = Arc::new(RecordingService::default());
        let effect = OpenWithEffect::with_wake(service.clone(), || {});
        let mut state = demo_state();
        let current = WorkspaceUri::new("file:///repo/zode").unwrap();
        let other = WorkspaceUri::new("file:///repo/other").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: current.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.active_workspace = Some(current.clone());

        for workspace_uri in [other, current.clone()] {
            assert!(consume_open_with_command(
                &state,
                &effect,
                &AppCommand::OpenWorkspaceExternally {
                    workspace_uri,
                    application: ExternalApplication::Finder,
                },
            ));
        }
        for _ in 0..100 {
            if service.0.lock().unwrap().len() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert_eq!(
            *service.0.lock().unwrap(),
            vec![(current, ExternalApplication::Finder)]
        );
    }

    #[test]
    fn selecting_an_application_never_reaches_the_open_effect() {
        let service = Arc::new(RecordingService::default());
        let effect = OpenWithEffect::with_wake(service.clone(), || {});
        let state = demo_state();

        assert!(!consume_open_with_command(
            &state,
            &effect,
            &AppCommand::SelectExternalApplication {
                application: ExternalApplication::Finder,
            },
        ));
        assert!(service.0.lock().unwrap().is_empty());
    }

    #[test]
    fn escape_and_outside_click_close_the_application_menu() {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.active_workspace = Some(workspace);
        state.open_with.menu_open = true;
        state.open_with.applications = LoadState::Ready(vec![ExternalApplication::Finder]);
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 800.0, Insets::ZERO);
        let anchor = ThreadHeader::layout(snapshot.layout.top_bar, &state)
            .open_with
            .unwrap();
        let menu =
            OpenWithMenu::menu_layout(anchor.rect, snapshot.layout.viewport, &state).unwrap();

        assert_eq!(
            open_with_escape_command(&state),
            Some(AppCommand::ToggleOpenWithMenu)
        );
        assert_eq!(
            open_with_outside_click_command(
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
            open_with_outside_click_command(
                &state,
                &snapshot,
                jian_widgets::Point2D::new(20.0, 700.0),
            ),
            Some(AppCommand::ToggleOpenWithMenu)
        );

        let narrow_snapshot = WorkspaceSnapshot::build(&state, 80.0, 800.0, Insets::ZERO);
        assert_eq!(
            open_with_outside_click_command(
                &state,
                &narrow_snapshot,
                jian_widgets::Point2D::new(20.0, 700.0),
            ),
            Some(AppCommand::ToggleOpenWithMenu)
        );
    }
}
