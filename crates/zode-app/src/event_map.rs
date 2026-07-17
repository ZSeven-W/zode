use jian_widgets::Point2D;
use winit::{
    event::Ime,
    keyboard::{Key as WinitKey, ModifiersState, NamedKey},
    window::ResizeDirection,
};
use zode_app_model::AppCommand;
use zode_app_ui::{
    ComposerOutcome, ImeEvent, Key, KeyEvent, Modifiers, SandboxSelection, WorkspaceLayout,
};

const RESIZE_RING: f32 = 6.0;
const WINDOW_CONTROLS_WIDTH: f32 = 160.0;

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
