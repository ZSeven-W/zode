//! Markdown -> ratatui Lines. Inline formatting (bold/italic/code),
//! headings, list items, and syntect-highlighted fenced code blocks.

use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Render markdown source into styled ratatui Lines.
pub fn render_markdown(src: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(src, options);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default().fg(theme.fg_text);
    let mut code_lang: Option<String> = None;
    let mut code_buffer = String::new();
    let mut table: Option<TableState> = None;

    for ev in parser {
        match ev {
            MdEvent::Start(Tag::Table(_)) => {
                flush_nonempty(&mut lines, &mut current);
                push_blank_if_needed(&mut lines);
                table = Some(TableState::default());
            }
            MdEvent::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    let rendered = render_table(table, theme);
                    if !rendered.is_empty() {
                        lines.extend(rendered);
                        lines.push(Line::from(""));
                    }
                }
            }
            MdEvent::Start(Tag::TableHead) => {
                if let Some(table) = &mut table {
                    table.in_header = true;
                }
            }
            MdEvent::End(TagEnd::TableHead) => {
                if let Some(table) = &mut table {
                    table.finish_pending_row();
                    table.in_header = false;
                    table.header_rows = table.rows.len();
                }
            }
            MdEvent::Start(Tag::TableRow) => {
                if let Some(table) = &mut table {
                    table.current_row.clear();
                }
            }
            MdEvent::End(TagEnd::TableRow) => {
                if let Some(table) = &mut table {
                    table.finish_row();
                }
            }
            MdEvent::Start(Tag::TableCell) => {
                if let Some(table) = &mut table {
                    table.current_cell.clear();
                }
            }
            MdEvent::End(TagEnd::TableCell) => {
                if let Some(table) = &mut table {
                    table.finish_cell();
                }
            }
            MdEvent::Start(Tag::Heading { .. }) => {
                style = Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD);
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                flush(&mut lines, &mut current);
                style = Style::default().fg(theme.fg_text);
            }
            MdEvent::Start(Tag::Strong) => style = style.add_modifier(Modifier::BOLD),
            MdEvent::End(TagEnd::Strong) => style = style.remove_modifier(Modifier::BOLD),
            MdEvent::Start(Tag::Emphasis) => style = style.add_modifier(Modifier::ITALIC),
            MdEvent::End(TagEnd::Emphasis) => style = style.remove_modifier(Modifier::ITALIC),
            MdEvent::Start(Tag::CodeBlock(kind)) => {
                flush(&mut lines, &mut current);
                code_lang = Some(match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                });
                code_buffer.clear();
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                if let Some(lang) = code_lang.take() {
                    lines.extend(highlight_code(&code_buffer, &lang, theme));
                }
            }
            MdEvent::Text(t) => {
                if code_lang.is_some() {
                    code_buffer.push_str(&t);
                } else if let Some(table) = &mut table {
                    table.current_cell.push_str(&t);
                } else {
                    current.push(Span::styled(t.to_string(), style));
                }
            }
            MdEvent::Code(t) => {
                if let Some(table) = &mut table {
                    table.current_cell.push_str(&t);
                } else {
                    current.push(Span::styled(
                        t.to_string(),
                        Style::default().fg(theme.accent_secondary),
                    ));
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => flush(&mut lines, &mut current),
            MdEvent::Start(Tag::Item) => {
                current.push(Span::styled("• ", Style::default().fg(theme.accent)))
            }
            MdEvent::End(TagEnd::Item) => flush(&mut lines, &mut current),
            MdEvent::End(TagEnd::Paragraph) if table.is_none() => {
                flush(&mut lines, &mut current);
                lines.push(Line::from(""));
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        flush(&mut lines, &mut current);
    }
    lines
}

fn flush(lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>) {
    lines.push(Line::from(std::mem::take(current)));
}

fn flush_nonempty(lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>) {
    if !current.is_empty() {
        flush(lines, current);
    }
}

fn push_blank_if_needed(lines: &mut Vec<Line<'static>>) {
    if lines.last().is_some_and(|line| !line.spans.is_empty()) {
        lines.push(Line::from(""));
    }
}

#[derive(Debug, Default)]
struct TableState {
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_header: bool,
    header_rows: usize,
}

impl TableState {
    fn finish_cell(&mut self) {
        self.current_row.push(self.current_cell.trim().to_string());
        self.current_cell.clear();
    }

    fn finish_row(&mut self) {
        self.rows.push(std::mem::take(&mut self.current_row));
        if self.in_header {
            self.header_rows = self.rows.len();
        }
    }

    fn finish_pending_row(&mut self) {
        if !self.current_row.is_empty() {
            self.finish_row();
        }
    }
}

fn render_table(table: TableState, theme: &Theme) -> Vec<Line<'static>> {
    let col_count = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return Vec::new();
    }

    let mut widths = vec![0usize; col_count];
    for row in &table.rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }

    let border = Style::default().fg(theme.separator);
    let head = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let body = Style::default().fg(theme.fg_text);
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        table_rule(&widths, "┌", "┬", "┐"),
        border,
    )));
    for (row_idx, row) in table.rows.iter().enumerate() {
        let cell_style = if row_idx < table.header_rows {
            head
        } else {
            body
        };
        out.push(render_table_row(row, &widths, cell_style, border));
        if row_idx + 1 == table.header_rows {
            out.push(Line::from(Span::styled(
                table_rule(&widths, "├", "┼", "┤"),
                border,
            )));
        }
    }
    out.push(Line::from(Span::styled(
        table_rule(&widths, "└", "┴", "┘"),
        border,
    )));
    out
}

fn render_table_row(
    row: &[String],
    widths: &[usize],
    cell_style: Style,
    border_style: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled("│ ", border_style)];
    for (idx, width) in widths.iter().copied().enumerate() {
        let cell = row.get(idx).map(String::as_str).unwrap_or("");
        spans.push(Span::styled(pad_display(cell, width), cell_style));
        spans.push(Span::styled(" │ ", border_style));
    }
    Line::from(spans)
}

fn table_rule(widths: &[usize], left: &str, join: &str, right: &str) -> String {
    let segments: Vec<String> = widths
        .iter()
        .map(|width| "─".repeat(width.saturating_add(2)))
        .collect();
    format!("{left}{}{right}", segments.join(join))
}

fn pad_display(s: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(s);
    if used >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - used))
    }
}

/// Syntect-highlight a code block; falls back to plain text if the language
/// is unknown.
fn highlight_code(code: &str, lang: &str, theme: &Theme) -> Vec<Line<'static>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let st = &THEME_SET.themes["base16-ocean.dark"];
    let mut hl = HighlightLines::new(syntax, st);
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = hl.highlight_line(line, &SYNTAX_SET).unwrap_or_default();
        let mut spans: Vec<Span<'static>> =
            vec![Span::styled("  ", Style::default().fg(theme.fg_subtle))];
        for (sty, text) in ranges {
            spans.push(Span::styled(
                text.trim_end_matches('\n').to_string(),
                Style::default().fg(ratatui::style::Color::Rgb(
                    sty.foreground.r,
                    sty.foreground.g,
                    sty.foreground.b,
                )),
            ));
        }
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    #[test]
    fn renders_heading_and_bold() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown("# Title\n\nsome **bold** text", &theme);
        let j = joined(&lines);
        assert!(j.contains("Title"));
        assert!(j.contains("bold"));
    }

    #[test]
    fn code_block_is_highlighted_not_dropped() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown("```rust\nfn main() {}\n```", &theme);
        assert!(joined(&lines).contains("fn main"));
    }

    #[test]
    fn list_items_render() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown("- one\n- two", &theme);
        let j = joined(&lines);
        assert!(j.contains("one"));
        assert!(j.contains("two"));
    }

    #[test]
    fn table_rows_render_as_terminal_table() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown(
            "| Type | 说明 | 示例 |\n|---|---|---|\n| Preference | 用户偏好 | 缩进 4 空格 |\n| Fact | 客观事实 | API 地址 |\n",
            &theme,
        );
        let rows: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        assert!(
            rows.iter()
                .any(|row| row.contains("Type") && row.contains("说明") && row.contains("示例")),
            "table header should render as one aligned terminal row: {rows:#?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("Preference") && row.contains("缩进 4 空格")),
            "table body should render as terminal rows: {rows:#?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("─")),
            "table separator should be rendered for scanability: {rows:#?}"
        );
    }

    #[test]
    fn table_renders_with_outer_borders_and_vertical_margin() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown(
            "Agent 通过 Tool 调用：\n\n| 方法 | 参数 | 返回 |\n|---|---|---|\n| memory_search | query | 匹配项 |\n检索和注入这块对吗？",
            &theme,
        );
        let rows: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        let top = rows
            .iter()
            .position(|row| row.starts_with("┌"))
            .expect("table should render a top border");
        let bottom = rows
            .iter()
            .position(|row| row.starts_with("└"))
            .expect("table should render a bottom border");

        assert!(
            rows[top].ends_with("┐"),
            "top border should close: {rows:#?}"
        );
        assert!(
            rows[bottom].ends_with("┘"),
            "bottom border should close: {rows:#?}"
        );
        assert_eq!(
            rows.get(top.saturating_sub(1)).map(String::as_str),
            Some("")
        );
        assert_eq!(rows.get(bottom + 1).map(String::as_str), Some(""));
    }

    #[test]
    fn yaml_frontmatter_code_block_renders_with_content() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown(
            "```yaml\n---\nid: \"a1b2c3d4\"\ntype: Fact\ntags: [\"port\", \"config\"]\n---\n```",
            &theme,
        );
        let j = joined(&lines);

        assert!(j.contains("id:"));
        assert!(j.contains("type:"));
        assert!(j.contains("tags:"));
        assert!(j.contains("a1b2c3d4"));
    }

    #[test]
    fn directory_tree_text_stays_line_oriented() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = render_markdown(
            "目录布局\n~/.zode/memory/\n├── global/\n│   ├── preferences.md\n└── projects/\n    └── <sha256>/\n",
            &theme,
        );
        let rows: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        assert!(rows.iter().any(|row| row.contains("~/.zode/memory/")));
        assert!(rows.iter().any(|row| row.contains("├── global/")));
        assert!(rows.iter().any(|row| row.contains("└── projects/")));
    }
}
