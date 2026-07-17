use jian_widgets::Point2D;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase as WinitTouchPhase},
    keyboard::{Key as WinitKey, ModifiersState, NamedKey},
    window::{ResizeDirection, Theme as WinitTheme},
};
use zode_app_model::{AppCommand, SystemTheme};
use zode_app_ui::{
    ComposerOutcome, FocusDirection, ImeEvent, Key, KeyEvent, Modifiers, PointerButton,
    PointerEvent, PointerEventKind, SandboxSelection, TouchEvent, TouchPhase, UnifiedInputEvent,
    WheelDeltaMode, WheelEvent, WorkspaceLayout,
};

const RESIZE_RING: f32 = 6.0;
const WINDOW_CONTROLS_WIDTH: f32 = 160.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRoute {
    Widget,
    TerminalPty,
    MoveFocus(FocusDirection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutPlatform {
    MacOS,
    Other,
}

pub fn map_pointer_move(position: PhysicalPosition<f64>, scale_factor: f64) -> UnifiedInputEvent {
    UnifiedInputEvent::Pointer(PointerEvent {
        position: logical_point(position, scale_factor),
        kind: PointerEventKind::Move,
        button: None,
    })
}

pub fn map_pointer_button(
    button: MouseButton,
    state: ElementState,
    cached_logical_position: Point2D,
) -> UnifiedInputEvent {
    UnifiedInputEvent::Pointer(PointerEvent {
        position: cached_logical_position,
        kind: match state {
            ElementState::Pressed => PointerEventKind::Press,
            ElementState::Released => PointerEventKind::Release,
        },
        button: Some(map_pointer_button_name(button)),
    })
}

pub fn map_touch(
    id: u64,
    position: PhysicalPosition<f64>,
    phase: WinitTouchPhase,
    scale_factor: f64,
) -> UnifiedInputEvent {
    UnifiedInputEvent::Touch(TouchEvent {
        id,
        position: logical_point(position, scale_factor),
        phase: match phase {
            WinitTouchPhase::Started => TouchPhase::Started,
            WinitTouchPhase::Moved => TouchPhase::Moved,
            WinitTouchPhase::Ended => TouchPhase::Ended,
            WinitTouchPhase::Cancelled => TouchPhase::Cancelled,
        },
    })
}

pub fn map_wheel(delta: MouseScrollDelta, scale_factor: f64) -> UnifiedInputEvent {
    let (delta_x, delta_y, mode) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (x, y, WheelDeltaMode::Line),
        MouseScrollDelta::PixelDelta(position) => {
            let point = logical_point(position, scale_factor);
            (point.x, point.y, WheelDeltaMode::Pixel)
        }
    };
    UnifiedInputEvent::Wheel(WheelEvent {
        delta_x,
        delta_y,
        mode,
    })
}

pub fn map_keyboard(
    logical_key: &WinitKey,
    modifiers: ModifiersState,
    pressed: bool,
) -> Option<UnifiedInputEvent> {
    map_key(logical_key, modifiers, pressed).map(UnifiedInputEvent::Keyboard)
}

pub fn map_ime_input(event: &Ime) -> UnifiedInputEvent {
    UnifiedInputEvent::Ime(map_ime(event))
}

pub fn route_key_event(event: &KeyEvent, terminal_focused: bool) -> InputRoute {
    if event.pressed && event.key == Key::Tab {
        if event.modifiers.contains(Modifiers::SHIFT) {
            return InputRoute::MoveFocus(FocusDirection::Backward);
        }
        if terminal_focused {
            return InputRoute::TerminalPty;
        }
        return InputRoute::MoveFocus(FocusDirection::Forward);
    }
    if terminal_focused {
        InputRoute::TerminalPty
    } else {
        InputRoute::Widget
    }
}

pub fn map_system_theme(theme: Option<WinitTheme>) -> SystemTheme {
    match theme {
        Some(WinitTheme::Dark) => SystemTheme::Dark,
        _ => SystemTheme::Light,
    }
}

pub fn map_key(
    logical_key: &WinitKey,
    modifiers: ModifiersState,
    pressed: bool,
) -> Option<KeyEvent> {
    let key = match logical_key {
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::PageUp) => Key::PageUp,
        WinitKey::Named(NamedKey::PageDown) => Key::PageDown,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Space) => Key::Character(" ".into()),
        WinitKey::Character(character) => Key::Character(character.to_string()),
        _ => return None,
    };
    Some(KeyEvent {
        key,
        modifiers: map_modifiers(modifiers),
        pressed,
    })
}

pub fn map_modifiers(modifiers: ModifiersState) -> Modifiers {
    let mut mapped = Modifiers::NONE;
    if modifiers.shift_key() {
        mapped = mapped | Modifiers::SHIFT;
    }
    if modifiers.control_key() {
        mapped = mapped | Modifiers::CONTROL;
    }
    if modifiers.alt_key() {
        mapped = mapped | Modifiers::ALT;
    }
    if modifiers.super_key() {
        mapped = mapped | Modifiers::SUPER;
    }
    mapped
}

pub fn map_ime(event: &Ime) -> ImeEvent {
    match event {
        Ime::Enabled => ImeEvent::Start,
        Ime::Preedit(text, cursor) => ImeEvent::Update {
            text: text.clone(),
            cursor: cursor.map(|(_, end)| end),
        },
        Ime::Commit(text) => ImeEvent::Commit(text.clone()),
        Ime::Disabled => ImeEvent::End,
    }
}

pub fn composer_outcome_command(outcome: ComposerOutcome) -> Option<AppCommand> {
    match outcome {
        ComposerOutcome::Ignored | ComposerOutcome::Edited => None,
        ComposerOutcome::Send(submission) => Some(AppCommand::Submit(submission.content)),
        ComposerOutcome::Steer(submission) => Some(AppCommand::Steer(submission.content)),
        ComposerOutcome::Stop => Some(AppCommand::Interrupt),
        ComposerOutcome::SetModel(model) => Some(AppCommand::SetModel(model)),
        ComposerOutcome::SetEffort(effort) => Some(AppCommand::SetEffort(effort)),
        ComposerOutcome::SetSandbox(SandboxSelection { mode, network }) => {
            Some(AppCommand::SetSandbox { mode, network })
        }
    }
}

pub fn terminal_shortcut_command(event: &KeyEvent) -> Option<AppCommand> {
    (event.pressed
        && event.modifiers.primary()
        && matches!(&event.key, Key::Character(value) if value == "`"))
    .then_some(AppCommand::OpenTerminal)
}

pub fn is_paste_shortcut(event: &KeyEvent, terminal_focused: bool) -> bool {
    let platform = if cfg!(target_os = "macos") {
        ShortcutPlatform::MacOS
    } else {
        ShortcutPlatform::Other
    };
    is_paste_shortcut_for(event, terminal_focused, platform)
}

pub fn is_paste_shortcut_for(
    event: &KeyEvent,
    terminal_focused: bool,
    platform: ShortcutPlatform,
) -> bool {
    if !event.pressed
        || !matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"))
    {
        return false;
    }
    if !terminal_focused {
        return event.modifiers.primary();
    }
    match platform {
        ShortcutPlatform::MacOS => event.modifiers.contains(Modifiers::SUPER),
        ShortcutPlatform::Other => {
            event.modifiers.contains(Modifiers::CONTROL)
                && event.modifiers.contains(Modifiers::SHIFT)
        }
    }
}

pub fn resize_direction(x: f32, y: f32, width: f32, height: f32) -> Option<ResizeDirection> {
    if ![x, y, width, height].iter().all(|value| value.is_finite())
        || width <= RESIZE_RING * 2.0
        || height <= RESIZE_RING * 2.0
    {
        return None;
    }
    let west = x <= RESIZE_RING;
    let east = x >= width - RESIZE_RING;
    let north = y <= RESIZE_RING;
    let south = y >= height - RESIZE_RING;
    match (west, east, north, south) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

/// The non-interactive center of the thread header is the native drag surface.
pub fn is_drag_region(point: Point2D, geometry: &WorkspaceLayout) -> bool {
    let header = geometry.top_bar;
    header.contains(point)
        && point.x >= header.origin.x + 48.0
        && point.x < header.origin.x + header.size.x - WINDOW_CONTROLS_WIDTH
}

fn logical_point(position: PhysicalPosition<f64>, scale_factor: f64) -> Point2D {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    Point2D::new((position.x / scale) as f32, (position.y / scale) as f32)
}

fn map_pointer_button_name(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Other(4),
        MouseButton::Forward => PointerButton::Other(5),
        MouseButton::Other(value) => PointerButton::Other(value),
    }
}
