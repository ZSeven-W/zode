use std::{collections::VecDeque, num::NonZeroU32, sync::Arc};

use jian_widgets::Rect;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::{
    reduce_terminal_command, AppCommand, ShellPage, SystemTheme, TerminalCommandOutcome,
    ZodeAppState,
};
use zode_app_ui::{
    ComposerController, Insets, TerminalGrid, TerminalPanel, TerminalPanelController,
    WorkspaceLayout, WorkspaceShell, ZodeTheme,
};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    event_bridge::AgentEventBridge,
    event_map::{
        composer_outcome_command, is_drag_region, map_ime, map_key, terminal_shortcut_command,
    },
    render::{FramePainter, NativeBackend, RasterSurface},
    services::LocalTerminalService,
    terminal_runtime::TerminalRuntime,
    window_state::{AppWake, WindowState},
};
use zode_node_protocol::{AgentEndpoint, NodeCapability};

pub fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<AppWake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut state = zode_app_model::demo_state();
    state
        .host
        .capabilities
        .capabilities
        .insert(NodeCapability::Terminal);
    let mut app = DesktopApp::new(state, proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
}

pub struct DesktopApp {
    app_state: ZodeAppState,
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
    pending_commands: VecDeque<AppCommand>,
}

impl DesktopApp {
    pub fn new(mut app_state: ZodeAppState, proxy: EventLoopProxy<AppWake>) -> Self {
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
        Self {
            app_state,
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
            pending_commands: VecDeque::new(),
        }
    }

    pub fn take_pending_commands(&mut self) -> Vec<AppCommand> {
        self.pending_commands.drain(..).collect()
    }

    /// Connect a live endpoint stream to the winit wake path. Call while the
    /// application Tokio runtime is entered, before `run_app` takes control.
    pub fn attach_endpoint(&mut self, endpoint: Arc<dyn AgentEndpoint>) {
        self.agent_events = Some(AgentEventBridge::spawn(endpoint, self.proxy.clone()));
    }

    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_some() {
            return Ok(());
        }
        let mut attributes = Window::default_attributes()
            .with_title("Zode")
            .with_inner_size(LogicalSize::new(1221.0, 992.0))
            .with_min_inner_size(LogicalSize::new(760.0, 560.0));
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes = attributes
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true)
                .with_traffic_light_inset(4.0);
        }
        #[cfg(not(target_os = "macos"))]
        {
            attributes = attributes.with_decorations(false);
        }

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        window.set_ime_allowed(true);
        let size = window.inner_size();
        self.window_state = WindowState::new(size.width, size.height, window.scale_factor());
        self.renderer = NativeBackend::new(self.window_state.scale_factor as f32);
        self.resize_terminal_grid();
        let context =
            softbuffer::Context::new(window.clone()).map_err(|error| error.to_string())?;
        let presenter = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| error.to_string())?;
        self.presenter = Some(presenter);
        self.window = Some(window.clone());
        window.request_redraw();
        Ok(())
    }

    fn redraw(&mut self) -> Result<(), String> {
        let (physical_width, physical_height) = (
            self.window_state.physical_width.max(1),
            self.window_state.physical_height.max(1),
        );
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
        canvas.scale((scale, scale));
        let (logical_width, logical_height) = self.window_state.logical_size();
        let theme = match self.app_state.host.system_theme {
            SystemTheme::Light => ZodeTheme::light(),
            SystemTheme::Dark => ZodeTheme::dark(),
        };
        {
            let mut painter = FramePainter::new(&mut self.renderer, canvas);
            WorkspaceShell::paint_with_composer_and_terminal_input(
                &mut painter,
                Rect::xywh(0.0, 0.0, logical_width, logical_height),
                Insets::ZERO,
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
        let (width, height) = self.window_state.logical_size();
        #[cfg(not(target_os = "macos"))]
        if let Some(direction) = resize_direction(
            self.window_state.cursor_logical.x,
            self.window_state.cursor_logical.y,
            width,
            height,
        ) {
            let _ = window.drag_resize_window(direction);
            return;
        }
        let geometry = WorkspaceLayout::compute(width, height, Insets::ZERO);
        if is_drag_region(self.window_state.cursor_logical, &geometry) {
            let _ = window.drag_window();
        }
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
            self.pending_commands.push_back(command);
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
            .current_session
            .as_ref()
            .and_then(|session| {
                self.app_state
                    .threads
                    .iter()
                    .find(|thread| &thread.session == session)
            })
            .and_then(|thread| crate::services::workspace_root(&thread.workspace_uri).ok())
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
        let (width, height) = self.window_state.logical_size();
        let geometry = WorkspaceLayout::compute(width, height, Insets::ZERO);
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
                self.pending_commands.push_back(command);
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
                if let Some(events) = self.agent_events.as_mut() {
                    events.drain_into(&mut self.app_state);
                }
                self.drain_terminal_output();
                self.sync_composer_busy();
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            AppWake::Close => event_loop.exit(),
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.window_state.physical_width = size.width.max(1);
                self.window_state.physical_height = size.height.max(1);
                self.resize_terminal_grid();
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_state.scale_factor = scale_factor;
                self.renderer = NativeBackend::new(scale_factor as f32);
                self.resize_terminal_grid();
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ThemeChanged(theme) => {
                self.app_state.host.system_theme = match theme {
                    winit::window::Theme::Dark => SystemTheme::Dark,
                    _ => SystemTheme::Light,
                };
                let _ = self.proxy.send_event(AppWake::Redraw);
            }
            WindowEvent::Focused(focused) => {
                self.app_state.composer.focused = focused;
                if self.app_state.shell.page == ShellPage::Terminal {
                    self.apply_terminal_command(AppCommand::SetTerminalFocus(focused));
                }
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(event) = map_key(
                    &event.logical_key,
                    self.modifiers,
                    event.state == ElementState::Pressed,
                ) {
                    if self.handle_terminal_key(&event) {
                        return;
                    }
                    if event.pressed {
                        let outcome = self.composer.key(event.key, event.modifiers);
                        self.apply_composer_outcome(outcome);
                    }
                }
            }
            WindowEvent::Ime(event) => {
                if self.app_state.shell.page == ShellPage::Terminal
                    && self.app_state.terminal.focused
                {
                    if let (Some(id), winit::event::Ime::Commit(text)) =
                        (self.app_state.terminal.active_id, &event)
                    {
                        self.apply_terminal_command(AppCommand::WriteTerminal {
                            id,
                            bytes: text.as_bytes().to_vec(),
                        });
                    }
                    return;
                }
                let event = map_ime(&event);
                let outcome = self.composer.ime(event);
                self.apply_composer_outcome(outcome);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.window_state
                    .set_cursor_physical(position.x, position.y);
                if self.app_state.shell.page == ShellPage::Terminal {
                    let changed = self.terminal_controller.pointer_move(
                        self.terminal_rect(),
                        self.window_state.cursor_logical,
                        &self.terminal_grid,
                        self.app_state.terminal.scroll_offset,
                    );
                    if changed {
                        self.window_state.dirty = true;
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.app_state.shell.page == ShellPage::Terminal {
                    let command = self.terminal_controller.pointer_down(
                        self.terminal_rect(),
                        self.window_state.cursor_logical,
                        &self.terminal_grid,
                        self.app_state.terminal.scroll_offset,
                    );
                    if let Some(command) = command {
                        self.apply_terminal_command(command);
                        return;
                    }
                }
                self.begin_window_gesture();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.terminal_controller.pointer_up(),
            WindowEvent::MouseWheel { delta, .. }
                if self.app_state.shell.page == ShellPage::Terminal =>
            {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 20.0,
                    MouseScrollDelta::PixelDelta(position) => -(position.y as f32),
                };
                let command = self.terminal_controller.scroll_command(
                    &self.app_state.terminal,
                    &self.terminal_grid,
                    self.terminal_rect().size.y,
                    delta,
                );
                self.apply_terminal_command(command);
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
        if self.window_state.dirty {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }
}
