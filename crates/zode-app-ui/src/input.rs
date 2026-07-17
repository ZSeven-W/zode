use jian_widgets::Point2D;

/// Mouse button vocabulary shared by every desktop host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Other(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub position: Point2D,
    pub kind: PointerEventKind,
    pub button: Option<PointerButton>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEventKind {
    Move,
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchEvent {
    pub id: u64,
    pub position: Point2D,
    pub phase: TouchPhase,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelEvent {
    pub delta_x: f32,
    pub delta_y: f32,
    pub mode: WheelDeltaMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDeltaMode {
    Pixel,
    Line,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Enter,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Escape,
    Character(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn primary(self) -> bool {
        self.contains(Self::CONTROL) || self.contains(Self::SUPER)
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
    pub pressed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    Start,
    Update { text: String, cursor: Option<usize> },
    Commit(String),
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnifiedInputEvent {
    Pointer(PointerEvent),
    Touch(TouchEvent),
    Wheel(WheelEvent),
    Keyboard(KeyEvent),
    Ime(ImeEvent),
}
