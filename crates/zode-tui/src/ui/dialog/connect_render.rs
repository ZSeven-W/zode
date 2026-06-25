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
    let text = if filter.is_empty() {
        "earch".to_string()
    } else {
        format!("earch {filter}")
    };
    Paragraph::new(Line::from(vec![
        Span::styled(
            "S",
            Style::default()
                .fg(theme.accent_secondary)
                .bg(theme.bg_secondary),
        ),
        Span::styled(
            text,
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
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
    // Reserve one column for the cursor when focused.
    let cursor_col = if focused { 1u16 } else { 0u16 };
    let value_cols = width.saturating_sub(label_len).saturating_sub(cursor_col) as usize;
    // Take the last `value_cols` chars so the end of a long value stays visible.
    let display: String = if value.chars().count() > value_cols {
        value
            .chars()
            .skip(value.chars().count() - value_cols)
            .collect()
    } else {
        let mut s = value.to_string();
        s.push_str(&" ".repeat(value_cols - value.chars().count()));
        s
    };
    let mut spans = vec![
        Span::styled(
            label_text,
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            display,
            Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
        ),
    ];
    if focused {
        // Reverse-video block: the cursor position.
        spans.push(Span::styled(
            " ",
            Style::default()
                .fg(theme.bg_secondary)
                .bg(theme.fg_text)
                .add_modifier(Modifier::REVERSED),
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
        ProviderSection::Popular => "Popular",
        ProviderSection::Providers => "Providers",
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
