//! Sidebar todo panel: a persistent borderless section beneath the sessions
//! list that renders the active tab's live `TodoWrite` list (status glyph plus
//! subject) under an activity/progress header. Read-only; the data comes from
//! the per-tab cached snapshot of `engine.todo_state`. Styled to match the
//! other sidebar sections (label line + content), no box border.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use zode_core::{TodoItem, TodoStatus};

use crate::theme::Theme;
use crate::ui::tabs::truncate_to_width;

/// Max item rows before the list collapses into a single "…+k more" row.
const CAP: usize = 8;
/// Rows reserved at the top of the sidebar (summary block + `sessions`
/// header + at least one tab row) before any space is given to the section.
const TOP_MIN: usize = 17;
/// Smallest useful section: one header row + one content row.
const MIN_BOX_H: usize = 2;

/// One rendered line of the box body.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TodoRow {
    Item { status: TodoStatus, subject: String },
    Overflow(usize),
    Empty,
}

pub(crate) fn glyph_for_status(s: TodoStatus) -> char {
    match s {
        TodoStatus::InProgress => '◐',
        TodoStatus::Completed => '✓',
        TodoStatus::Cancelled => '✗',
        TodoStatus::Pending => '○',
        _ => '○',
    }
}

fn color_for_status(s: TodoStatus, theme: &Theme) -> ratatui::style::Color {
    match s {
        TodoStatus::InProgress => theme.accent,
        TodoStatus::Completed | TodoStatus::Cancelled => theme.fg_subtle,
        TodoStatus::Pending => theme.fg_text,
        _ => theme.fg_text,
    }
}

/// `(label, count)` for the header row: e.g. `("Todo · running…", "1/3")`.
/// Unpadded — `render_todo_box` lays them out (label left, count right).
pub(crate) fn todo_title(todos: &[TodoItem], busy: bool) -> (String, String) {
    let activity = if busy { "running…" } else { "idle" };
    let done = todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    (
        format!("Todo · {activity}"),
        format!("{done}/{}", todos.len()),
    )
}

/// Build the ordered body rows. `subject_width` is the column budget for a
/// subject after its glyph + space; `max_rows` is how many content rows fit
/// inside the borders (always treated as ≥1).
pub(crate) fn todo_rows(todos: &[TodoItem], subject_width: usize, max_rows: usize) -> Vec<TodoRow> {
    if todos.is_empty() {
        return vec![TodoRow::Empty];
    }
    let max_rows = max_rows.max(1);
    let mk = |t: &TodoItem| TodoRow::Item {
        status: t.status,
        subject: truncate_to_width(&t.subject, subject_width),
    };
    if todos.len() <= max_rows {
        return todos.iter().map(mk).collect();
    }
    let shown = max_rows.saturating_sub(1);
    let mut rows: Vec<TodoRow> = todos.iter().take(shown).map(mk).collect();
    rows.push(TodoRow::Overflow(todos.len() - shown));
    rows
}

pub(crate) fn row_text(row: &TodoRow) -> String {
    match row {
        TodoRow::Item { status, subject } => {
            format!("{} {}", glyph_for_status(*status), subject)
        }
        TodoRow::Overflow(k) => format!("…+{k} more"),
        TodoRow::Empty => "no active todos".to_string(),
    }
}

/// Rows to allocate beneath the sessions list for the section, or 0 to skip it
/// (sidebar too short). Clamps so the top region keeps `TOP_MIN` rows. One row
/// is the header; the rest are item rows.
pub(crate) fn todo_box_height(item_count: usize, sidebar_height: usize) -> usize {
    let available = sidebar_height.saturating_sub(TOP_MIN);
    if available < MIN_BOX_H {
        return 0;
    }
    let content_rows = if item_count == 0 {
        1
    } else {
        item_count.min(CAP)
    };
    (content_rows + 1).min(available)
}

/// The header row: `todo · <activity>` (accent, bold) on the left and the
/// `done/total` count (subtle) right-aligned, padded to the sidebar width.
fn header_line(todos: &[TodoItem], busy: bool, width: usize, theme: &Theme) -> Line<'static> {
    let (label, count) = todo_title(todos, busy);
    let left_w = 1 + UnicodeWidthStr::width(label.as_str()); // leading pad + label
    let right_w = UnicodeWidthStr::width(count.as_str()) + 1; // count + trailing pad
    let pad = width.saturating_sub(left_w + right_w);
    let bg = Style::default().bg(theme.bg_secondary);
    Line::from(vec![
        Span::styled(
            format!(" {label}"),
            bg.fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad), bg),
        Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
    ])
}

pub(crate) fn render_todo_box(
    f: &mut Frame,
    area: Rect,
    todos: &[TodoItem],
    busy: bool,
    theme: &Theme,
) {
    if (area.height as usize) < MIN_BOX_H || area.width < 4 {
        return;
    }
    let width = area.width as usize;
    // The first row is the header; the rest hold item rows.
    let item_rows = (area.height as usize).saturating_sub(1);
    // Each item row is " " (leading pad) + glyph + " " + subject, so the
    // subject budget is the width minus those 3 chrome columns. Off by one
    // here clips the trailing ellipsis on truncated subjects.
    let subject_width = width.saturating_sub(3);

    let mut lines: Vec<Line> = vec![header_line(todos, busy, width, theme)];
    lines.extend(
        todo_rows(todos, subject_width, item_rows)
            .iter()
            .map(|row| {
                let style = match row {
                    TodoRow::Item { status, .. } => Style::default()
                        .fg(color_for_status(*status, theme))
                        .bg(theme.bg_secondary),
                    _ => Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
                };
                Line::from(Span::styled(format!(" {}", row_text(row)), style))
            }),
    );

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::{TodoItem, TodoStatus};

    fn item(subject: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            subject: subject.to_string(),
            description: None,
            status,
            id: None,
        }
    }

    #[test]
    fn glyphs_match_status() {
        assert_eq!(glyph_for_status(TodoStatus::InProgress), '◐');
        assert_eq!(glyph_for_status(TodoStatus::Pending), '○');
        assert_eq!(glyph_for_status(TodoStatus::Completed), '✓');
        assert_eq!(glyph_for_status(TodoStatus::Cancelled), '✗');
    }

    #[test]
    fn title_shows_activity_and_progress() {
        let todos = vec![
            item("a", TodoStatus::Completed),
            item("b", TodoStatus::InProgress),
            item("c", TodoStatus::Pending),
        ];
        assert_eq!(
            todo_title(&todos, true),
            ("Todo · running…".to_string(), "1/3".to_string())
        );
        assert_eq!(
            todo_title(&todos, false),
            ("Todo · idle".to_string(), "1/3".to_string())
        );
        assert_eq!(
            todo_title(&[], false),
            ("Todo · idle".to_string(), "0/0".to_string())
        );
    }

    #[test]
    fn empty_list_yields_placeholder_row() {
        let rows = todo_rows(&[], 20, 5);
        assert_eq!(rows, vec![TodoRow::Empty]);
        assert_eq!(row_text(&rows[0]), "no active todos");
    }

    #[test]
    fn rows_render_glyph_and_truncated_subject() {
        let todos = vec![item(
            "wire snapshot into the sidebar",
            TodoStatus::InProgress,
        )];
        let rows = todo_rows(&todos, 10, 5);
        assert_eq!(rows.len(), 1);
        // truncate_to_width keeps 9 cols of body + '…' => "wire snap…"
        assert_eq!(row_text(&rows[0]), "◐ wire snap…");
    }

    #[test]
    fn overflow_collapses_to_more_row() {
        let todos: Vec<TodoItem> = (0..10)
            .map(|i| item(&format!("t{i}"), TodoStatus::Pending))
            .collect();
        let rows = todo_rows(&todos, 20, 3);
        assert_eq!(rows.len(), 3); // 2 items + 1 overflow
        assert_eq!(row_text(&rows[2]), "…+8 more");
    }

    #[test]
    fn render_clips_subject_without_dropping_ellipsis() {
        // Regression: the rendered row is " " + glyph + " " + subject, so the
        // subject budget must be inner_width - 3. If it's only -2, a maxed-out
        // subject overflows the inner width by one column and ratatui clips the
        // trailing '…'. Render at the real 34-col sidebar width and assert the
        // ellipsis survives.
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let todos = vec![item(&"a".repeat(40), TodoStatus::Pending)];
        let backend = ratatui::backend::TestBackend::new(34, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_todo_box(f, f.area(), &todos, false, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        // Borderless: width = 34, subject budget = 31 -> 30 'a' + '…' fills the
        // item row exactly (no border to clip the trailing ellipsis).
        assert!(content.contains(&format!("{}…", "a".repeat(30))));
        assert!(!content.contains(&"a".repeat(31)));
    }

    #[test]
    fn box_height_clamps_and_skips() {
        assert_eq!(todo_box_height(3, 40), 4); // header + 3 items
        assert_eq!(todo_box_height(0, 40), 2); // header + placeholder row
        assert_eq!(todo_box_height(3, 18), 0); // no room above TOP_MIN -> skip
        assert_eq!(todo_box_height(50, 40), 9); // header + CAP(8) items
    }
}
