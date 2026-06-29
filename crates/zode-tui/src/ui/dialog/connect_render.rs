//! Pure render helpers for `ConnectDialog`. Extracted to keep `connect.rs`
//! within the 800-line file-size budget.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::theme::Theme;

use super::connect::ProviderSection;

/// Centred modal rectangle within `area`, clamped to the terminal size.
pub(crate) fn modal_area(
    area: ratatui::layout::Rect,
    target_width: u16,
    target_height: u16,
) -> ratatui::layout::Rect {
    let max_w = area.width.saturating_sub(6);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(target_width).max(max_w.min(44));
    let height = max_h.min(target_height).max(max_h.min(8));
    ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// Inner area with a 3-column left/right margin and 1-row top/bottom margin.
pub(crate) fn inner_area(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect {
        x: area.x.saturating_add(3),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(6),
        height: area.height.saturating_sub(2),
    }
}

pub(crate) fn header_line(title: &'static str, width: u16, theme: &Theme) -> Paragraph<'static> {
    let title_width = title.chars().count() as u16;
    let gap = width.saturating_sub(title_width.saturating_add(3)) as usize;
    Paragraph::new(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(gap), Style::default().bg(theme.bg_secondary)),
        Span::styled(
            "esc",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

pub(crate) fn search_line(filter: &str, theme: &Theme) -> Paragraph<'static> {
    // The whole word is translated (no split-letter accent), so it works for
    // non-Latin scripts like the CJK "搜索".
    let label = zode_core::i18n::t("Search");
    let text = if filter.is_empty() {
        label.to_string()
    } else {
        format!("{label} {filter}")
    };
    Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default()
            .fg(theme.accent_secondary)
            .bg(theme.bg_secondary),
    )]))
    .style(Style::default().bg(theme.bg_secondary))
}

/// Render a labelled value row with an optional focused-cursor block appended.
pub(crate) fn field_line_focused(
    label: &str,
    value: &str,
    width: u16,
    theme: &Theme,
    focused: bool,
) -> Line<'static> {
    let label_text = format!("{label} ");
    let label_len = label_text.chars().count() as u16;
    // Reserve one column for the cursor block when focused.
    let cursor_col = if focused { 1u16 } else { 0u16 };

    // The focused row uses the accent color (label + a solid block cursor) so
    // it's unmistakable which field is active; idle rows dim their label.
    let label_style = if focused {
        Style::default()
            .fg(theme.accent)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)
    };

    // Degenerate width (very narrow terminal): not enough room for the label
    // plus cursor. Emit only a clipped label so the line never exceeds `width`.
    if width < label_len.saturating_add(cursor_col) {
        let clipped: String = label_text.chars().take(width as usize).collect();
        return Line::from(Span::styled(clipped, label_style));
    }

    let value_cols = width.saturating_sub(label_len).saturating_sub(cursor_col) as usize;
    // Take the last `value_cols` chars so the end of a long value stays visible.
    let shown: String = if value.chars().count() > value_cols {
        value
            .chars()
            .skip(value.chars().count() - value_cols)
            .collect()
    } else {
        value.to_string()
    };
    // Pad AFTER the cursor (below), not before — otherwise the cursor block
    // floats at the far-right edge instead of marking the insertion point.
    let pad = value_cols.saturating_sub(shown.chars().count());

    let mut spans = vec![
        Span::styled(label_text, label_style),
        Span::styled(
            shown,
            Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
        ),
    ];
    if focused {
        // Solid accent block marking the text-insertion point.
        spans.push(Span::styled(
            " ",
            Style::default().fg(theme.bg_secondary).bg(theme.accent),
        ));
    }
    if pad > 0 {
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(theme.bg_secondary),
        ));
    }
    Line::from(spans)
}

pub(crate) fn footer_line(
    key: &'static str,
    label: &'static str,
    theme: &Theme,
) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            key,
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

pub(crate) fn section_title(section: ProviderSection) -> &'static str {
    match section {
        ProviderSection::Configured => zode_core::i18n::t("Configured"),
        ProviderSection::Providers => zode_core::i18n::t("Providers"),
    }
}

pub(crate) fn fixed_width(value: &str, width: usize) -> String {
    let mut out: String = value.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

pub(crate) fn pad_to_width(value: String, width: u16) -> String {
    let width = width as usize;
    let mut out: String = value.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

pub(crate) fn mask_secret(secret: &str) -> String {
    "*".repeat(secret.chars().count())
}

/// Human-readable label for a `ProviderKind` displayed in the Type field.
pub(crate) fn kind_label(kind: zode_core::config::ProviderKind) -> String {
    use zode_core::config::ProviderKind;
    match kind {
        ProviderKind::Anthropic => "‹ anthropic ›".to_string(),
        ProviderKind::Openai => "‹ openai ›".to_string(),
        ProviderKind::Ollama => "‹ ollama ›".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"))
    }

    fn line_width(line: &Line) -> usize {
        line.spans.iter().map(|s| s.content.chars().count()).sum()
    }

    #[test]
    fn field_line_never_exceeds_width_even_when_narrow() {
        let th = theme();
        // "max output " is the widest label (11 cols); sweep widths that can't
        // fit it so the degenerate path is exercised.
        for w in 0u16..14 {
            for focused in [true, false] {
                let line = field_line_focused("max output", "12345", w, &th, focused);
                assert!(
                    line_width(&line) <= w as usize,
                    "width {w} focused={focused}: line is {} cols, exceeds {w}",
                    line_width(&line)
                );
            }
        }
    }

    #[test]
    fn field_line_fills_exact_width_when_roomy() {
        let th = theme();
        assert_eq!(
            line_width(&field_line_focused("model", "abc", 40, &th, true)),
            40
        );
        assert_eq!(
            line_width(&field_line_focused("model", "abc", 40, &th, false)),
            40
        );
    }
}
