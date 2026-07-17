/// Responsive layout classes used by the platform-independent UI model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutClass {
    Wide,
    Compact,
    Phone,
}

impl LayoutClass {
    /// Classifies a viewport using the desktop design breakpoints.
    pub fn for_width(width: f32) -> Self {
        if width >= 960.0 {
            Self::Wide
        } else if width >= 720.0 {
            Self::Compact
        } else {
            Self::Phone
        }
    }
}
