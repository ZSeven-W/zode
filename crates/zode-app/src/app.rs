use std::{num::NonZeroU32, sync::Arc};

use jian_widgets::Rect;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::{
    reduce_terminal_command, AppCommand, ShellPage, TerminalCommandOutcome, ZodeAppState,
};
use zode_app_ui::{
    accessibility_tree, ComposerController, Insets, TerminalGrid, TerminalPanel,
    TerminalPanelController, WidgetId, WorkspaceShell, WorkspaceSnapshot, TERMINAL_ID,
};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    bootstrap_state::load_initial_state,
    clipboard::{ClipboardService, NativeClipboardService},
    command_bridge::CommandBridge,
    event_bridge::AgentEventBridge,
    event_map::{
        composer_outcome_command, is_drag_region, map_ime_input, map_keyboard, map_pointer_button,
        map_pointer_move, map_system_theme, map_touch, map_wheel, terminal_shortcut_command,
    },
    render::{FramePainter, NativeBackend, RasterSurface},
    services::LocalTerminalService,
    terminal_runtime::TerminalRuntime,
    window_bootstrap::hidden_window_attributes_for_placement,
    window_state::{
        AppWake, WindowGeometry, WindowState, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    },
    work_area::{resolve_startup_placement, PlatformWorkAreaProvider},
};
use zode_app_runtime::{path_to_workspace_uri, AppStateStore, LocalAppRuntime};
use zode_core::{bootstrap::AppBootstrap, config::ConfigManager};
use zode_node_protocol::{AgentEndpoint, NodeCapability};

mod interaction;

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

    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_some() {
            return Ok(());
        }
        let provider = PlatformWorkAreaProvider::from_event_loop(event_loop);
        let (placement, work_area_warning) =
            resolve_startup_placement(self.window_geometry, &provider);
        if let Some(error) = work_area_warning {
            eprintln!(
                "zode-app: saved window position was not restored because work-area discovery is unavailable: {error}"
            );
        }
        let attributes = hidden_window_attributes_for_placement(placement);

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        window.set_ime_allowed(false);
        self.app_state.host.system_theme = map_system_theme(window.theme());
        let size = window.inner_size();
        self.window_state = WindowState::new(size.width, size.height, window.scale_factor());
        self.renderer = NativeBackend::new(self.window_state.scale_factor as f32);
        self.rebuild_frame_snapshot();
        self.resize_terminal_grid();

        let wake_proxy = self.proxy.clone();
        let wake = Arc::new(move || {
            let _ = wake_proxy.send_event(AppWake::Redraw);
        });
        let a11y = AccessibilityHost::install_before_show(
            &window,
            &self.frame_snapshot,
            self.window_state.scale_factor,
            wake,
            placement.maximized(),
        )
        .map_err(|error| error.to_string())?;
        let context =
            softbuffer::Context::new(window.clone()).map_err(|error| error.to_string())?;
        let presenter = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| error.to_string())?;
        self.presenter = Some(presenter);
        self.a11y = Some(a11y);
        self.window = Some(window.clone());
        self.record_window_geometry();
        window.request_redraw();
        Ok(())
    }

    fn redraw(&mut self) -> Result<(), String> {
        let (physical_width, physical_height) = (
            self.window_state.physical_width.max(1),
            self.window_state.physical_height.max(1),
        );
        self.rebuild_frame_snapshot();
        let required = (physical_width, physical_height);
        if self.raster.as_ref().map(RasterSurface::size) != Some(required) {
            self.raster = Some(
                RasterSurface::new(physical_width, physical_height)
                    .map_err(|error| error.to_string())?,
            );
        }
        let raster = self.raster.as_mut().expect("raster initialized");
        let canvas = raster.canvas();
        canvas.reset_matrix();
        canvas.clear(skia_safe::Color::WHITE);
        let scale = self.window_state.scale_factor as f32;
        if let Some(a11y) = self.a11y.as_mut() {
            a11y.push(accessibility_tree(
                &self.frame_snapshot,
                self.window_state.scale_factor,
            ));
        }
        canvas.scale((scale, scale));
        let theme = crate::preferences::theme_for_state(&self.app_state);
        {
            let mut painter = FramePainter::new(&mut self.renderer, canvas);
            WorkspaceShell::paint_snapshot_with_composer_and_terminal_input(
                &mut painter,
                &self.frame_snapshot,
                &self.app_state,
                self.composer.input_state(),
                &self.terminal_grid,
                self.terminal_controller.selection(),
                &theme,
            );
        }

        let mut rgba = vec![0_u8; physical_width as usize * physical_height as usize * 4];
        if !raster.read_rgba8(&mut rgba) {
            return Err("Skia framebuffer read failed".into());
        }
        let presenter = self
            .presenter
            .as_mut()
            .ok_or_else(|| "presenter is not initialized".to_owned())?;
        presenter
            .resize(
                NonZeroU32::new(physical_width).expect("positive width"),
                NonZeroU32::new(physical_height).expect("positive height"),
            )
            .map_err(|error| error.to_string())?;
        let mut buffer = presenter.buffer_mut().map_err(|error| error.to_string())?;
        for (index, pixel) in buffer.iter_mut().enumerate() {
            let offset = index * 4;
            *pixel = (u32::from(rgba[offset]) << 16)
                | (u32::from(rgba[offset + 1]) << 8)
                | u32::from(rgba[offset + 2]);
        }
        if let Some(window) = self.window.as_ref() {
            window.pre_present_notify();
        }
        buffer.present().map_err(|error| error.to_string())?;
        self.window_state.dirty = false;
        Ok(())
    }

    fn begin_window_gesture(&self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        #[cfg(not(target_os = "macos"))]
        {
            let (width, height) = self.window_state.logical_size();
            if let Some(direction) = resize_direction(
                self.window_state.cursor_logical.x,
                self.window_state.cursor_logical.y,
                width,
                height,
            ) {
                let _ = window.drag_resize_window(direction);
                return;
            }
        }
        let geometry = self.frame_snapshot.layout;
        if is_drag_region(self.window_state.cursor_logical, &geometry) {
            let _ = window.drag_window();
        }
    }

    fn rebuild_frame_snapshot(&mut self) {
        let (width, height) = self.window_state.logical_size();
        let mut snapshot = WorkspaceSnapshot::build(
            &self.app_state,
            width,
            height,
            self.window_state.safe_area_insets,
        );
        snapshot.focused = self
            .focused_widget
            .filter(|focused| snapshot.node(*focused).is_some())
            .or(snapshot.focused);
        self.focused_widget = snapshot.focused;
        self.frame_snapshot = snapshot;
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

    fn apply_terminal_command(&mut self, command: AppCommand) {
        if command == AppCommand::OpenTerminal {
            let _ = reduce_terminal_command(&mut self.app_state, command);
            if self.terminal_runtime.active_id().is_none()
                && self.app_state.terminal.unavailable_reason.is_none()
            {
                let cwd = self.terminal_cwd();
                let (cols, rows) = self.terminal_grid.size();
                match self.terminal_runtime.open(
                    &cwd,
                    u16::try_from(cols).unwrap_or(u16::MAX),
                    u16::try_from(rows).unwrap_or(u16::MAX),
                ) {
                    Ok(id) => self.app_state.terminal.active_id = Some(id),
                    Err(error) => {
                        self.app_state.terminal.unavailable_reason = Some(error.to_string())
                    }
                }
            }
            self.rebuild_frame_snapshot();
            self.set_focused_widget(Some(TERMINAL_ID));
        } else {
            match reduce_terminal_command(&mut self.app_state, command.clone()) {
                TerminalCommandOutcome::NeedsEffect => {
                    let result = match command {
                        AppCommand::WriteTerminal { id, bytes } => {
                            self.terminal_runtime.write(id, bytes)
                        }
                        AppCommand::ResizeTerminal { id, cols, rows } => {
                            self.terminal_runtime.resize(id, cols, rows)
                        }
                        AppCommand::CloseTerminal(id) => {
                            let result = self.terminal_runtime.close(id);
                            if result.is_ok() {
                                self.app_state.terminal.active_id = None;
                                self.app_state.terminal.open = false;
                                self.app_state.terminal.focused = false;
                            }
                            result
                        }
                        _ => Ok(()),
                    };
                    if let Err(error) = result {
                        self.app_state.terminal.unavailable_reason = Some(error.to_string());
                    }
                }
                TerminalCommandOutcome::Applied => {}
                TerminalCommandOutcome::Ignored => return,
            }
        }
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn terminal_cwd(&self) -> std::path::PathBuf {
        self.app_state
            .active_available_workspace()
            .and_then(|workspace| crate::services::workspace_root(workspace).ok())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    fn drain_terminal_output(&mut self) {
        let mut changed = false;
        for output in self.terminal_runtime.drain_output() {
            match output {
                Ok(bytes) => {
                    self.terminal_grid.feed(&bytes);
                    changed = true;
                }
                Err(error) => self.app_state.terminal.unavailable_reason = Some(error.to_string()),
            }
        }
        if changed && self.app_state.terminal.follow_tail {
            self.app_state.terminal.scroll_offset = TerminalPanel::tail_offset(
                self.terminal_grid.line_count(),
                self.terminal_rect().size.y,
            );
        }
        match self.terminal_runtime.reap_finished() {
            Ok(Some(id)) if self.app_state.terminal.active_id == Some(id) => {
                self.app_state.terminal.active_id = None;
                self.app_state.terminal.open = false;
                self.app_state.terminal.focused = false;
            }
            Ok(_) => {}
            Err(error) => self.app_state.terminal.unavailable_reason = Some(error.to_string()),
        }
    }

    fn terminal_rect(&self) -> Rect {
        let geometry = self.frame_snapshot.layout;
        Rect::xywh(
            geometry.transcript.origin.x,
            geometry.transcript.origin.y,
            geometry.transcript.size.x,
            geometry.composer.origin.y + geometry.composer.size.y - geometry.transcript.origin.y,
        )
    }

    fn handle_terminal_key(&mut self, event: &zode_app_ui::KeyEvent) -> bool {
        if let Some(command) = terminal_shortcut_command(event) {
            self.apply_terminal_command(command);
            return true;
        }
        if self.app_state.shell.page != ShellPage::Terminal || !self.app_state.terminal.focused {
            return false;
        }
        if TerminalPanelController::is_copy_shortcut(event) {
            if let Some(command) = self.terminal_controller.copy_command(&self.terminal_grid) {
                self.enqueue_command(command);
            }
            return true;
        }
        if let Some(command) = self
            .terminal_controller
            .key_command(&self.app_state.terminal, event)
        {
            self.apply_terminal_command(command);
        }
        true
    }

    fn resize_terminal_grid(&mut self) {
        let rect = self.terminal_rect();
        let cols = ((rect.size.x - 16.0).max(8.0) / 8.0).floor() as usize;
        let rows = (rect.size.y.max(20.0) / 20.0).floor() as usize;
        if self.terminal_grid.size() == (cols, rows) {
            return;
        }
        self.terminal_grid.resize(cols, rows);
        if self.app_state.terminal.follow_tail {
            self.app_state.terminal.scroll_offset =
                TerminalPanel::tail_offset(self.terminal_grid.line_count(), rect.size.y);
        }
        if let Some(id) = self.app_state.terminal.active_id {
            self.apply_terminal_command(AppCommand::ResizeTerminal {
                id,
                cols: u16::try_from(cols).unwrap_or(u16::MAX),
                rows: u16::try_from(rows).unwrap_or(u16::MAX),
            });
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
