use std::{collections::VecDeque, num::NonZeroU32, sync::Arc};

use jian_widgets::Rect;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};
use zode_app_model::{AppCommand, SystemTheme, ZodeAppState};
use zode_app_ui::{ComposerController, Insets, WorkspaceLayout, WorkspaceShell, ZodeTheme};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    event_bridge::AgentEventBridge,
    event_map::{composer_outcome_command, is_drag_region, map_ime, map_key},
    render::{FramePainter, NativeBackend, RasterSurface},
    window_state::{AppWake, WindowState},
};
use zode_node_protocol::AgentEndpoint;

pub fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<AppWake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = DesktopApp::new(zode_app_model::demo_state(), proxy);
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
    modifiers: ModifiersState,
    pending_commands: VecDeque<AppCommand>,
}

impl DesktopApp {
    pub fn new(app_state: ZodeAppState, proxy: EventLoopProxy<AppWake>) -> Self {
        let mut composer = ComposerController::new(app_state.composer.draft.clone());
        let busy = app_state
            .current_session
            .as_ref()
            .and_then(|session| app_state.transcripts.get(session))
            .is_some_and(|transcript| transcript.busy);
        composer.set_busy(busy);
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
            WorkspaceShell::paint_with_composer_input(
                &mut painter,
                Rect::xywh(0.0, 0.0, logical_width, logical_height),
                Insets::ZERO,
                &self.app_state,
                self.composer.input_state(),
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
                self.window_state.dirty = true;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_state.scale_factor = scale_factor;
                self.renderer = NativeBackend::new(scale_factor as f32);
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
                    if event.pressed {
                        let outcome = self.composer.key(event.key, event.modifiers);
                        self.apply_composer_outcome(outcome);
                    }
                }
            }
            WindowEvent::Ime(event) => {
                let event = map_ime(&event);
                let outcome = self.composer.ime(event);
                self.apply_composer_outcome(outcome);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.window_state
                    .set_cursor_physical(position.x, position.y);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.begin_window_gesture(),
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
