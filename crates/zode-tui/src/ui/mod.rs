//! TUI widgets and rendering helpers.

pub mod autocomplete;
pub mod chat;
pub mod dialog;
pub mod diff;
pub mod help;
pub mod input;
pub mod layout;
pub mod markdown;
pub mod mention;
pub mod status;
pub mod subagents_sidebar;
pub mod tabs;
pub mod toast;
pub mod todo;

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// A centered rect `pct_x`% wide × `pct_y`% tall within `area`.
pub fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}
