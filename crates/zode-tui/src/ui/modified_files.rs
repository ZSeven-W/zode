//! Collapsible "modified files" sidebar section: the git working tree's
//! uncommitted changes with per-file `+added -removed` counts (numstat).
//! Borderless, styled to match the other sidebar sections; the ▼/▶ header
//! row is a click target (fold toggle).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use zode_core::GitFileStat;

use crate::theme::Theme;
use crate::ui::tabs::fit_line_to_width;

/// Max file rows shown before collapsing into a "…+k more" row.
const CAP: usize = 8;

/// Rows this section occupies, or 0 when there are no modified files: a blank
/// separator + a header, plus up to `CAP` file rows when expanded.
pub(crate) fn section_height(count: usize, collapsed: bool) -> usize {
    match (count, collapsed) {
        (0, _) => 0,
        (_, true) => 2,
        (n, false) => 2 + n.min(CAP),
    }
}

/// Index of the "…+k more" row within this section's lines, when present —
/// the click target that opens the full-list overlay. Layout: blank(0),
/// header(1), `CAP - 1` file rows, then the overflow row.
pub(crate) fn overflow_row_index(count: usize, collapsed: bool) -> Option<usize> {
    (!collapsed && count > CAP).then_some(CAP + 1)
}

/// Keep the END of a path (the filename) when truncating: `…c/main/app.rs`.
fn truncate_path_left(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".repeat(max_width.min(1));
    }
    let body_width = max_width - 1;
    let mut used = 0;
    let mut tail = String::new();
    for ch in text.chars().rev() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > body_width {
            break;
        }
        tail.insert(0, ch);
        used += w;
    }
    format!("…{tail}")
}

/// Build the section's lines (empty when there are no modified files).
/// `width` is the full sidebar row width.
pub(crate) fn section_lines(
    files: &[GitFileStat],
    collapsed: bool,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if files.is_empty() {
        return Vec::new();
    }
    let bg = Style::default().bg(theme.bg_secondary);
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(" ".repeat(width), bg))];

    // Header: " ▼ modified files" (accent, bold) + right-aligned total.
    let arrow = if collapsed { '▶' } else { '▼' };
    let label = format!(" {arrow} {}", crate::tr("modified files"));
    let count = files.len().to_string();
    let pad = width.saturating_sub(
        UnicodeWidthStr::width(label.as_str()) + UnicodeWidthStr::width(count.as_str()) + 1,
    );
    lines.push(fit_line_to_width(
        Line::from(vec![
            Span::styled(label, bg.fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" ".repeat(pad), bg),
            Span::styled(format!("{count} "), bg.fg(theme.fg_subtle)),
        ]),
        width,
        bg,
    ));
    if collapsed {
        return lines;
    }

    // File rows: " path" + right-aligned "+a -d" (untracked/binary: no counts).
    let (shown, overflow) = if files.len() > CAP {
        (CAP - 1, files.len() - (CAP - 1))
    } else {
        (files.len(), 0)
    };
    for f in files.iter().take(shown) {
        let added = f.added.map(|n| format!("+{n}")).unwrap_or_default();
        let removed = f.removed.map(|n| format!("-{n}")).unwrap_or_default();
        let stats_w = match (added.is_empty(), removed.is_empty()) {
            (true, true) => 0,
            (false, false) => added.len() + 1 + removed.len() + 1,
            _ => added.len() + removed.len() + 1,
        };
        let path_w = width.saturating_sub(2 + stats_w);
        let path = truncate_path_left(&f.path, path_w);
        let pad = width.saturating_sub(1 + UnicodeWidthStr::width(path.as_str()) + stats_w);
        let mut spans = vec![
            Span::styled(format!(" {path}"), bg.fg(theme.fg_text)),
            Span::styled(" ".repeat(pad), bg),
        ];
        if !added.is_empty() {
            spans.push(Span::styled(added, bg.fg(Color::Green)));
            spans.push(Span::styled(" ", bg));
        }
        if !removed.is_empty() {
            spans.push(Span::styled(removed, bg.fg(Color::Red)));
            spans.push(Span::styled(" ", bg));
        }
        lines.push(fit_line_to_width(Line::from(spans), width, bg));
    }
    if overflow > 0 {
        lines.push(fit_line_to_width(
            Line::from(Span::styled(
                format!("   …+{overflow} {}", crate::tr("more")),
                bg.fg(theme.fg_subtle),
            )),
            width,
            bg,
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    fn line_width(line: &Line) -> usize {
        line.spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum()
    }

    fn stat(path: &str, added: Option<u32>, removed: Option<u32>) -> GitFileStat {
        GitFileStat {
            path: path.into(),
            added,
            removed,
        }
    }

    #[test]
    fn empty_list_renders_nothing() {
        assert_eq!(section_height(0, false), 0);
        assert!(section_lines(&[], false, 30, &theme()).is_empty());
    }

    #[test]
    fn header_rows_and_counts_render() {
        let files = vec![
            stat("crates/zode-tui/src/app.rs", Some(4), Some(1)),
            stat("new_file.rs", None, None),
        ];
        assert_eq!(section_height(2, false), 4); // blank + header + 2 rows
        let j = joined(&section_lines(&files, false, 34, &theme()));
        assert!(j.contains("▼ modified files"));
        assert!(j.contains('2')); // total in the header
        assert!(j.contains("app.rs"));
        assert!(j.contains("+4"));
        assert!(j.contains("-1"));
        assert!(j.contains("new_file.rs"));
    }

    #[test]
    fn collapsed_keeps_only_the_header() {
        let files = vec![stat("a.rs", Some(1), Some(0))];
        assert_eq!(section_height(1, true), 2);
        let lines = section_lines(&files, true, 34, &theme());
        assert_eq!(lines.len(), 2); // blank + header
        let j = joined(&lines);
        assert!(j.contains("▶ modified files"));
        assert!(!j.contains("a.rs"));
    }

    #[test]
    fn overflow_collapses_to_more_row() {
        let files: Vec<GitFileStat> = (0..12)
            .map(|i| stat(&format!("f{i}.rs"), None, None))
            .collect();
        assert_eq!(section_height(12, false), 2 + CAP);
        let j = joined(&section_lines(&files, false, 34, &theme()));
        assert!(j.contains(&format!("…+{} more", 12 - (CAP - 1))));
    }

    #[test]
    fn long_paths_keep_their_filename_tail() {
        assert_eq!(truncate_path_left("short.rs", 20), "short.rs");
        let t = truncate_path_left("crates/zode-tui/src/ui/modified_files.rs", 18);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("modified_files.rs"));
        assert!(UnicodeWidthStr::width(t.as_str()) <= 18);
    }

    #[test]
    fn narrow_rows_never_exceed_the_section_width() {
        let files: Vec<GitFileStat> = (0..10)
            .map(|i| {
                stat(
                    &format!("crates/zode-tui/src/ui/very-long-file-{i}.rs"),
                    Some(1),
                    Some(1),
                )
            })
            .collect();
        let lines = section_lines(&files, false, 8, &theme());
        for line in &lines {
            assert!(
                line_width(line) <= 8,
                "line width {} exceeded 8: {line:?}",
                line_width(line)
            );
        }
    }
}
