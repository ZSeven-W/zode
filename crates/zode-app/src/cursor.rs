pub use jian_core::CursorHint;
use jian_widgets::Point2D;
use winit::window::CursorIcon;
use zode_app_ui::WorkspaceSnapshot;

pub fn cursor_icon_for_hint(hint: CursorHint) -> CursorIcon {
    match hint {
        CursorHint::Default => CursorIcon::Default,
        CursorHint::Pointer => CursorIcon::Pointer,
        CursorHint::Move => CursorIcon::Move,
        CursorHint::Grabbing => CursorIcon::Grabbing,
        CursorHint::Grab => CursorIcon::Grab,
        CursorHint::Crosshair => CursorIcon::Crosshair,
        CursorHint::Text => CursorIcon::Text,
        CursorHint::NotAllowed => CursorIcon::NotAllowed,
        CursorHint::ResizeEw => CursorIcon::EwResize,
        CursorHint::ResizeNs => CursorIcon::NsResize,
        CursorHint::ResizeNwse => CursorIcon::NwseResize,
        CursorHint::ResizeNesw => CursorIcon::NeswResize,
        CursorHint::Rotate => CursorIcon::Alias,
    }
}

pub fn cursor_hint_at(snapshot: &WorkspaceSnapshot, point: Point2D) -> CursorHint {
    snapshot
        .hit_test(point)
        .and_then(|id| snapshot.node(id))
        .map_or(CursorHint::Default, |node| node.cursor)
}
