use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::ZodeAppState;
use zode_app_ui::{
    ComposerController, Insets, TerminalGrid, TerminalPanelController, WidgetId, WorkspaceSnapshot,
};

use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    bootstrap_state::load_initial_state,
    clipboard::{ClipboardService, NativeClipboardService},
    command_bridge::CommandBridge,
    event_bridge::AgentEventBridge,
    event_map::{
        composer_outcome_command, map_ime_input, map_keyboard, map_pointer_button,
        map_pointer_move, map_touch, map_wheel,
    },
    render::{NativeBackend, RasterSurface},
    services::LocalTerminalService,
    terminal_runtime::TerminalRuntime,
    window_state::{
        AppWake, WindowGeometry, WindowState, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
};
use zode_app_runtime::{path_to_workspace_uri, AppStateStore, LocalAppRuntime};
use zode_core::{bootstrap::AppBootstrap, config::ConfigManager};
use zode_node_protocol::{AgentEndpoint, NodeCapability};

mod interaction;
mod terminal;
mod window;

pub fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let cwd = std::env::current_dir()?;
    let startup_workspace = path_to_workspace_uri(&cwd)?;
    let bootstrap = tokio_runtime.block_on(AppBootstrap::new(cwd).resolve())?;
    let config_dir = ConfigManager::config_dir()?;
    let runtime = {
        let _guard = tokio_runtime.enter();
        LocalAppRuntime::new(config_dir, bootstrap, 256)?
    };
    let endpoint: Arc<dyn AgentEndpoint> = runtime.endpoint();
    let state = tokio_runtime.block_on(load_initial_state(
        endpoint.as_ref(),
        runtime.capabilities().clone(),
        startup_workspace,
    ))?;

    let event_loop = EventLoop::<AppWake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = DesktopApp::new(state, proxy);
    let _guard = tokio_runtime.enter();
    app.attach_endpoint(endpoint);
    event_loop.run_app(&mut app)?;
    Ok(())
}

pub struct DesktopApp {
    app_state: ZodeAppState,
    a11y: Option<AccessibilityHost>,
    window: Option<Arc<Window>>,
    presenter: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    raster: Option<RasterSurface>,
    renderer: NativeBackend,
    window_state: WindowState,
    proxy: EventLoopProxy<AppWake>,
    agent_events: Option<AgentEventBridge>,
    composer: ComposerController,
    terminal_grid: TerminalGrid,
    terminal_controller: TerminalPanelController,
    terminal_runtime: TerminalRuntime,
    modifiers: ModifiersState,
    command_bridge: Option<CommandBridge>,
    settings_touch: crate::input_dispatch::SettingsTouchTracker,
    frame_snapshot: WorkspaceSnapshot,
    focused_widget: Option<WidgetId>,
    window_focused: bool,
    clipboard: Option<Arc<dyn ClipboardService>>,
    app_state_store: Option<AppStateStore>,
    window_geometry: Option<WindowGeometry>,
}

impl DesktopApp {
    pub fn new(mut app_state: ZodeAppState, proxy: EventLoopProxy<AppWake>) -> Self {
        let app_state_store = match AppStateStore::from_default_config() {
            Ok(store) => Some(store),
            Err(error) => {
                eprintln!("zode-app: UI state persistence is unavailable: {error}");
                None
            }
        };
        let persisted = app_state_store
            .as_ref()
            .and_then(|store| match store.load() {
                Ok(state) => Some(state),
                Err(error) => {
                    eprintln!(
                        "zode-app: persisted UI state could not be loaded and was left untouched: {error}"
                    );
                    None
                }
            });
        if let Some(persisted) = persisted.as_ref() {
            app_state.ui_preferences = persisted.ui_preferences.clone();
            for project in &mut app_state.projects {
                project.expanded = !persisted
                    .collapsed_workspaces
                    .contains(project.workspace_uri.as_str());
            }
            if let Some(last_session) = persisted.last_session.as_ref() {
                let restored = app_state
                    .threads
                    .iter()
                    .find(|thread| {
                        &thread.session.session_id == last_session
                            && app_state.available_workspace(&thread.workspace_uri)
                    })
                    .map(|thread| (thread.session.clone(), thread.workspace_uri.clone()));
                if let Some((session, workspace_uri)) = restored {
                    app_state.current_session = Some(session);
                    app_state.active_workspace = Some(workspace_uri);
                }
            }
        }
        app_state.composer.focused = false;
        app_state.terminal.focused = false;
        if !app_state
            .host
            .capabilities
            .capabilities
            .contains(&NodeCapability::Terminal)
            && app_state.terminal.unavailable_reason.is_none()
        {
            app_state.terminal.unavailable_reason =
                Some("Terminal is unavailable on this node.".into());
        }
        let mut composer = ComposerController::new(app_state.composer.draft.clone());
        let busy = app_state
            .current_session
            .as_ref()
            .and_then(|session| app_state.transcripts.get(session))
            .is_some_and(|transcript| transcript.busy);
        composer.set_busy(busy);
        let terminal_proxy = proxy.clone();
        let terminal_runtime =
            TerminalRuntime::new(Arc::new(LocalTerminalService::new()), move || {
                let _ = terminal_proxy.send_event(AppWake::Redraw);
            });
        let frame_snapshot = WorkspaceSnapshot::build(
            &app_state,
            DEFAULT_WINDOW_WIDTH as f32,
            DEFAULT_WINDOW_HEIGHT as f32,
            Insets::ZERO,
        );
        let focused_widget = frame_snapshot
            .focused
            .or_else(|| frame_snapshot.focusable_ids().first().copied());
        let window_geometry = persisted.and_then(|persisted| persisted.window_geometry);
        let clipboard = match NativeClipboardService::new() {
            Ok(clipboard) => Some(Arc::new(clipboard) as Arc<dyn ClipboardService>),
            Err(error) => {
                eprintln!("zode-app: native clipboard is unavailable: {error}");
                None
            }
        };
        Self {
            app_state,
            a11y: None,
            window: None,
            presenter: None,
            raster: None,
            renderer: NativeBackend::new(1.0),
            window_state: WindowState::new(1221, 992, 1.0),
            proxy,
            agent_events: None,
            composer,
            terminal_grid: TerminalGrid::new(80, 24),
            terminal_controller: TerminalPanelController::default(),
            terminal_runtime,
            modifiers: ModifiersState::empty(),
            command_bridge: None,
            settings_touch: crate::input_dispatch::SettingsTouchTracker::default(),
            frame_snapshot,
            focused_widget,
            window_focused: false,
            clipboard,
            app_state_store,
            window_geometry,
        }
    }

    pub fn set_clipboard_service(&mut self, clipboard: Arc<dyn ClipboardService>) {
        self.clipboard = Some(clipboard);
    }

    /// Connect a live endpoint stream to the winit wake path. Call while the
    /// application Tokio runtime is entered, before `run_app` takes control.
    pub fn attach_endpoint(&mut self, endpoint: Arc<dyn AgentEndpoint>) {
        self.agent_events = Some(AgentEventBridge::spawn(
            endpoint.clone(),
            self.proxy.clone(),
        ));
        self.command_bridge = Some(CommandBridge::spawn(endpoint, self.proxy.clone()));
    }

    fn sync_composer_busy(&mut self) {
        let busy = self
            .app_state
            .current_session
            .as_ref()
            .and_then(|session| self.app_state.transcripts.get(session))
            .is_some_and(|transcript| transcript.busy);
        self.composer.set_busy(busy);
    }

    fn apply_composer_outcome(&mut self, outcome: zode_app_ui::ComposerOutcome) {
        self.app_state.composer.draft = self.composer.text().to_owned();
        if let Some(command) = composer_outcome_command(outcome) {
            self.enqueue_command(command);
        }
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<AppWake> for DesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.open_window(event_loop) {
            eprintln!("zode-app: failed to open window: {error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppWake) {
        match event {
            AppWake::Redraw => {
                self.drain_accessibility_actions();
                let events_applied = self
                    .agent_events
                    .as_mut()
                    .map_or(0, |events| events.drain_into(&mut self.app_state));
                let commands_applied = self
                    .command_bridge
                    .as_mut()
                    .map_or(0, |commands| commands.drain_into(&mut self.app_state));
                if events_applied + commands_applied > 0 {
                    self.rebuild_frame_snapshot();
                }
                self.drain_terminal_output();
                self.sync_composer_busy();
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            AppWake::Close => {
                self.record_window_geometry();
                self.persist_ui_state();
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.record_window_geometry();
                self.persist_ui_state();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.window_state.physical_width = size.width.max(1);
                self.window_state.physical_height = size.height.max(1);
                self.rebuild_frame_snapshot();
                self.resize_terminal_grid();
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
                self.request_redraw();
            }
            WindowEvent::Moved(_) => {
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_state.scale_factor = scale_factor;
                self.renderer = NativeBackend::new(scale_factor as f32);
                self.rebuild_frame_snapshot();
                self.resize_terminal_grid();
                self.record_window_geometry();
                self.update_accessibility_window_bounds();
                self.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => {
                self.app_state.host.system_theme = crate::event_map::map_system_theme(Some(theme));
                let _ = self.proxy.send_event(AppWake::Redraw);
            }
            WindowEvent::Focused(focused) => {
                if let Some(a11y) = self.a11y.as_mut() {
                    a11y.set_window_focused(focused);
                }
                self.sync_window_focus(focused);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(input) = map_keyboard(
                    &event.logical_key,
                    self.modifiers,
                    event.state == ElementState::Pressed,
                ) {
                    self.handle_unified_input(input);
                }
            }
            WindowEvent::Ime(event) => {
                self.handle_unified_input(map_ime_input(&event));
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_unified_input(map_pointer_move(
                    position,
                    self.window_state.scale_factor,
                ));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_unified_input(map_pointer_button(
                    button,
                    state,
                    self.window_state.cursor_logical,
                ));
            }
            WindowEvent::Touch(touch) => {
                self.handle_unified_input(map_touch(
                    touch.id,
                    touch.location,
                    touch.phase,
                    self.window_state.scale_factor,
                ));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_unified_input(map_wheel(delta, self.window_state.scale_factor));
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    eprintln!("zode-app: redraw failed: {error}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.drain_accessibility_actions();
        if self.window_state.dirty {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.record_window_geometry();
        self.persist_ui_state();
    }
}
