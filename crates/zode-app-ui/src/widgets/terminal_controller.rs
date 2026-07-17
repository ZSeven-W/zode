use jian_widgets::{Point2D, Rect};
use zode_app_model::{AppCommand, TerminalState};

use crate::{Key, KeyEvent, Modifiers};

use super::terminal_grid::{
    CellPosition, TerminalGrid, TerminalSelection, CELL_WIDTH, LINE_HEIGHT, TEXT_INSET,
};
use super::terminal_panel::TerminalPanel;

const TAIL_EPSILON: f32 = 1.0;

/// Translates terminal panel input into platform-neutral application commands.
#[derive(Debug, Default)]
pub struct TerminalPanelController {
    anchor: Option<CellPosition>,
    selection: Option<TerminalSelection>,
}

impl TerminalPanelController {
    pub fn selection(&self) -> Option<TerminalSelection> {
        self.selection
    }

    pub fn pointer_down(
        &mut self,
        rect: Rect,
        point: Point2D,
        grid: &TerminalGrid,
        scroll_offset: f32,
    ) -> Option<AppCommand> {
        let position = hit_test(rect, point, grid, scroll_offset)?;
        self.anchor = Some(position);
        self.selection = Some(single_cell_selection(position));
        Some(AppCommand::SetTerminalFocus(true))
    }

    pub fn pointer_move(
        &mut self,
        rect: Rect,
        point: Point2D,
        grid: &TerminalGrid,
        scroll_offset: f32,
    ) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let Some(current) = hit_test(rect, point, grid, scroll_offset) else {
            return false;
        };
        self.selection = Some(inclusive_selection(anchor, current));
        true
    }

    pub fn pointer_up(&mut self) {
        self.anchor = None;
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.selection = None;
    }

    pub fn copy_command(&self, grid: &TerminalGrid) -> Option<AppCommand> {
        let selected = TerminalPanel::copy_selection(grid, self.selection?);
        (!selected.is_empty()).then_some(AppCommand::CopyText(selected))
    }

    pub fn is_copy_shortcut(event: &KeyEvent) -> bool {
        if !event.pressed
            || !matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("c"))
        {
            return false;
        }
        event.modifiers.contains(Modifiers::SUPER)
            || (event.modifiers.contains(Modifiers::CONTROL)
                && event.modifiers.contains(Modifiers::SHIFT))
    }

    pub fn scroll_command(
        &self,
        state: &TerminalState,
        grid: &TerminalGrid,
        viewport_height: f32,
        delta: f32,
    ) -> AppCommand {
        let content_height = grid.line_count() as f32 * LINE_HEIGHT;
        let max_offset = (content_height - viewport_height.max(0.0)).max(0.0);
        let offset = (state.scroll_offset + delta).clamp(0.0, max_offset);
        AppCommand::SetTerminalScroll {
            offset,
            follow_tail: max_offset - offset <= TAIL_EPSILON,
        }
    }

    pub fn key_command(&self, state: &TerminalState, event: &KeyEvent) -> Option<AppCommand> {
        if !event.pressed || !state.focused || event.modifiers.contains(Modifiers::SUPER) {
            return None;
        }
        let id = state.active_id?;
        let bytes = if event.modifiers.contains(Modifiers::CONTROL) {
            match &event.key {
                Key::Character(text) => vec![control_byte(text)?],
                _ => return None,
            }
        } else {
            match &event.key {
                Key::Character(text) if !event.modifiers.contains(Modifiers::ALT) => {
                    text.as_bytes().to_vec()
                }
                Key::Enter => b"\r".to_vec(),
                Key::Backspace => vec![0x7f],
                Key::Delete => b"\x1b[3~".to_vec(),
                Key::ArrowLeft => b"\x1b[D".to_vec(),
                Key::ArrowRight => b"\x1b[C".to_vec(),
                Key::Home => b"\x1b[H".to_vec(),
                Key::End => b"\x1b[F".to_vec(),
                Key::Tab => b"\t".to_vec(),
                Key::Escape => b"\x1b".to_vec(),
                _ => return None,
            }
        };
        Some(AppCommand::WriteTerminal { id, bytes })
    }
}

fn control_byte(text: &str) -> Option<u8> {
    let byte = text.as_bytes().first().copied()?;
    (text.len() == 1 && byte.is_ascii_alphabetic()).then(|| byte.to_ascii_uppercase() & 0x1f)
}

fn hit_test(
    rect: Rect,
    point: Point2D,
    grid: &TerminalGrid,
    scroll_offset: f32,
) -> Option<CellPosition> {
    if !rect.contains(point) {
        return None;
    }
    let local_x = point.x - rect.origin.x - TEXT_INSET;
    if local_x < 0.0 {
        return None;
    }
    let col = (local_x / CELL_WIDTH).floor() as usize;
    let row = ((point.y - rect.origin.y + scroll_offset.max(0.0)) / LINE_HEIGHT).floor() as usize;
    let (cols, _) = grid.size();
    (col < cols && row < grid.line_count()).then_some(CellPosition { row, col })
}

fn single_cell_selection(position: CellPosition) -> TerminalSelection {
    TerminalSelection {
        start: position,
        end: CellPosition {
            row: position.row,
            col: position.col + 1,
        },
    }
}

fn inclusive_selection(left: CellPosition, right: CellPosition) -> TerminalSelection {
    if (left.row, left.col) <= (right.row, right.col) {
        TerminalSelection {
            start: left,
            end: CellPosition {
                row: right.row,
                col: right.col + 1,
            },
        }
    } else {
        TerminalSelection {
            start: right,
            end: CellPosition {
                row: left.row,
                col: left.col + 1,
            },
        }
    }
}
