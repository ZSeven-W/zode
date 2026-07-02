//! Collapsible "todo" sidebar section: the active tab's live `TodoWrite`
//! list (status glyph plus subject) under an activity/progress header.
//! Flows with the other sections (subagents/modified files) — shown only
//! when the list is non-empty, never pinned. Read-only; the data comes from
//! the per-tab cached snapshot of `engine.todo_state`. The ▼/▶ header row is
//! a click target (fold toggle).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use zode_core::{TodoItem, TodoStatus};

use crate::theme::Theme;
use crate::ui::tabs::truncate_to_width;

/// Max item rows before the list collapses into a single "…+k more" row.
const CAP: usize = 8;

/// One rendered line of the section body.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TodoRow {
    Item { status: TodoStatus, subject: String },
    Overflow(usize),
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
pub(crate) fn todo_title(todos: &[TodoItem], busy: bool) -> (String, String) {
    let activity = if busy {
        format!("{}…", crate::tr("running"))
    } else {
        crate::tr("idle").to_string()
    };
    let done = todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    (
        format!("{} · {activity}", crate::tr("Todo")),
        format!("{done}/{}", todos.len()),
    )
}

/// Build the ordered body rows for a non-empty list. `subject_width` is the
/// column budget for a subject after its glyph + space; `max_rows` is how many
/// content rows may render (always treated as ≥1).
pub(crate) fn todo_rows(todos: &[TodoItem], subject_width: usize, max_rows: usize) -> Vec<TodoRow> {
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
        TodoRow::Overflow(k) => format!("…+{k} {}", crate::tr("more")),
    }
}

/// Rows this section occupies, or 0 when there are no todos: a blank
/// separator + a header, plus up to `CAP` item rows when expanded.
pub(crate) fn section_height(count: usize, collapsed: bool) -> usize {
    match (count, collapsed) {
        (0, _) => 0,
        (_, true) => 2,
        (n, false) => 2 + n.min(CAP),
    }
}

/// Build the section's lines (empty when there are no todos). `width` is the
/// full sidebar row width.
pub(crate) fn section_lines(
    todos: &[TodoItem],
    busy: bool,
    collapsed: bool,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if todos.is_empty() {
        return Vec::new();
    }
    let bg = Style::default().bg(theme.bg_secondary);
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(" ".repeat(width), bg))];

    // Header: " ▼ todo · activity" (accent, bold) + right-aligned done/total.
    let arrow = if collapsed { '▶' } else { '▼' };
    let (label, count) = todo_title(todos, busy);
    let label = format!(" {arrow} {label}");
    let pad = width.saturating_sub(
        UnicodeWidthStr::width(label.as_str()) + UnicodeWidthStr::width(count.as_str()) + 1,
    );
    lines.push(Line::from(vec![
        Span::styled(label, bg.fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(pad), bg),
        Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
    ]));
    if collapsed {
        return lines;
    }

    // Item rows: " " (leading pad) + glyph + " " + subject (truncated).
    let subject_width = width.saturating_sub(3);
    for row in todo_rows(todos, subject_width, CAP) {
        let style = match &row {
            TodoRow::Item { status, .. } => bg.fg(color_for_status(*status, theme)),
            TodoRow::Overflow(_) => bg.fg(theme.fg_subtle),
        };
        lines.push(Line::from(Span::styled(
            format!(" {}", row_text(&row)),
            style,
        )));
    }
    lines
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

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
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
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert_eq!(section_height(0, false), 0);
        assert_eq!(section_height(0, true), 0);
        assert!(section_lines(&[], false, false, 30, &theme()).is_empty());
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
        assert_eq!(section_height(10, false), 2 + CAP);
    }

    #[test]
    fn section_renders_header_and_items() {
        let todos = vec![
            item("read tabs.rs", TodoStatus::Completed),
            item("wire snapshot", TodoStatus::InProgress),
        ];
        assert_eq!(section_height(2, false), 4); // blank + header + 2 rows
        let j = joined(&section_lines(&todos, true, false, 34, &theme()));
        assert!(j.contains("▼ Todo · running…"));
        assert!(j.contains("1/2"));
        assert!(j.contains("wire snapshot"));
    }

    #[test]
    fn collapsed_keeps_only_the_header() {
        let todos = vec![item("a", TodoStatus::Pending)];
        assert_eq!(section_height(1, true), 2);
        let lines = section_lines(&todos, false, true, 34, &theme());
        assert_eq!(lines.len(), 2); // blank + header
        let j = joined(&lines);
        assert!(j.contains("▶ Todo · idle"));
        assert!(!j.contains("○ a"));
    }
}
