//! Full modified-files overlay: opened from the sidebar section's "…+k more"
//! row, listing EVERY uncommitted working-tree change with its +added/-removed
//! counts (the sidebar shows only the first few). Esc/q closes; ↑/↓ and the
//! mouse wheel scroll. Row formatting is pure so it can be unit-tested
//! without a Frame.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;
use zode_core::GitFileStat;

use crate::theme::Theme;
use crate::ui::centered;

pub struct FilesPanel {
    scroll: usize,
}

impl Default for FilesPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// `(path, "+a", "-d")` cells for one row; count cells are empty for
/// untracked/binary files (no numstat).
pub(crate) fn row_cells(f: &GitFileStat) -> (String, String, String) {
    (
        f.path.clone(),
        f.added.map(|n| format!("+{n}")).unwrap_or_default(),
        f.removed.map(|n| format!("-{n}")).unwrap_or_default(),
    )
}

impl FilesPanel {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize, total: usize) {
        self.scroll = (self.scroll + n).min(total.saturating_sub(1));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, files: &[GitFileStat], theme: &Theme) {
        let popup = centered(area, 70, 70);
        f.render_widget(Clear, popup);

        let title = format!(" {} ({}) ", crate::tr("modified files"), files.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.separator))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        if inner.height == 0 || inner.width < 8 {
            return;
        }

        // One row per file; a footer hint takes the last inner row.
        let body_rows = (inner.height as usize).saturating_sub(1);
        self.scroll = self.scroll.min(files.len().saturating_sub(body_rows));
        let width = inner.width as usize;

        let mut lines: Vec<Line<'static>> = files
            .iter()
            .skip(self.scroll)
            .take(body_rows)
            .map(|stat| {
                let (path, added, removed) = row_cells(stat);
                let stats_w = match (added.is_empty(), removed.is_empty()) {
                    (true, true) => 0,
                    (false, false) => added.len() + 1 + removed.len() + 1,
                    _ => added.len() + removed.len() + 1,
                };
                let path_w = width.saturating_sub(2 + stats_w);
                let path = crate::ui::tabs::truncate_to_width(&path, path_w);
                let pad = width.saturating_sub(1 + UnicodeWidthStr::width(path.as_str()) + stats_w);
                let mut spans = vec![
                    Span::styled(format!(" {path}"), Style::default().fg(theme.fg_text)),
                    Span::raw(" ".repeat(pad)),
                ];
                if !added.is_empty() {
                    spans.push(Span::styled(added, Style::default().fg(Color::Green)));
                    spans.push(Span::raw(" "));
                }
                if !removed.is_empty() {
                    spans.push(Span::styled(removed, Style::default().fg(Color::Red)));
                    spans.push(Span::raw(" "));
                }
                Line::from(spans)
            })
            .collect();
        let footer = format!(" ↑↓ {} · esc {}", crate::tr("scroll"), crate::tr("close"));
        lines.push(Line::from(Span::styled(
            footer,
            Style::default().fg(theme.fg_subtle),
        )));
        f.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(path: &str, added: Option<u32>, removed: Option<u32>) -> GitFileStat {
        GitFileStat {
            path: path.into(),
            added,
            removed,
        }
    }

    #[test]
    fn row_cells_format_counts_and_leave_untracked_bare() {
        assert_eq!(
            row_cells(&stat("a.rs", Some(4), Some(1))),
            ("a.rs".into(), "+4".into(), "-1".into())
        );
        assert_eq!(
            row_cells(&stat("new.rs", None, None)),
            ("new.rs".into(), String::new(), String::new())
        );
    }

    #[test]
    fn scroll_clamps_to_list() {
        let mut p = FilesPanel::new();
        p.scroll_down(3, 10);
        assert_eq!(p.scroll, 3);
        p.scroll_down(100, 10);
        assert_eq!(p.scroll, 9);
        p.scroll_up(4);
        assert_eq!(p.scroll, 5);
        p.scroll_up(100);
        assert_eq!(p.scroll, 0);
    }

    #[test]
    fn renders_all_rows_with_counts() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let files: Vec<GitFileStat> = (0..12)
            .map(|i| stat(&format!("crates/f{i}.rs"), Some(i), Some(1)))
            .collect();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut panel = FilesPanel::new();
        terminal
            .draw(|f| panel.render(f, f.area(), &files, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("modified files (12)"));
        assert!(content.contains("crates/f0.rs"));
        assert!(content.contains("crates/f11.rs")); // ALL rows visible (12 fit)
        assert!(content.contains("+11"));
    }
}
