//! Markdown -> ratatui Lines. Inline formatting (bold/italic/code),
//! headings, list items, and syntect-highlighted fenced code blocks.

use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::Theme;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Render markdown source into styled ratatui Lines.
pub fn render_markdown(src: &str, theme: &Theme) -> Vec<Line<'static>> {
    let parser = Parser::new(src);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default().fg(theme.fg_text);
    let mut code_lang: Option<String> = None;
    let mut code_buffer = String::new();

    for ev in parser {
        match ev {
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
                } else {
                    current.push(Span::styled(t.to_string(), style));
                }
            }
            MdEvent::Code(t) => {
                current.push(Span::styled(
                    t.to_string(),
                    Style::default().fg(theme.accent_secondary),
                ));
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => flush(&mut lines, &mut current),
            MdEvent::Start(Tag::Item) => {
                current.push(Span::styled("• ", Style::default().fg(theme.accent)))
            }
            MdEvent::End(TagEnd::Item) => flush(&mut lines, &mut current),
            MdEvent::End(TagEnd::Paragraph) => {
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
}
