use jian_widgets::{
    components::markdown::{parse_blocks, parse_inline, MdBlock, MdRun},
    Color, Painter, Point2D, Rect, TextLayout,
};

use crate::{RectExt, TypographyRole, ZodeTheme};

#[path = "code-syntax.rs"]
mod code_syntax;
use code_syntax::TokenClass;

const MARKDOWN_LIMIT: usize = 50_000;
const BODY_LINE_HEIGHT: f32 = 22.0;
const BLOCK_GAP: f32 = 8.0;
const BULLET_INDENT: f32 = 20.0;
const USER_HORIZONTAL_PADDING: f32 = 16.0;
const USER_VERTICAL_PADDING: f32 = 9.0;
const USER_MIN_HEIGHT: f32 = 40.0;
const USER_MAX_WIDTH_FRACTION: f32 = 0.77;
const USER_MEASURE_SLOP: f32 = 8.0;

/// Vertical space reserved below every user bubble and every assistant
/// message, whether or not its hover-revealed (or, for the newest assistant
/// message, persistent) action row is currently painted. Reserving this
/// unconditionally - as part of the item's own measured height rather than
/// the inter-item gap - is what makes hover a paint-only concern: it never
/// changes `estimated_item_height`, so it never invalidates
/// `TranscriptState`'s layout cache. See `message_actions::ROW_HEIGHT`.
pub(super) const ACTION_ROW_RESERVED: f32 = super::message_actions::ROW_HEIGHT;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConversationBlock {
    Heading { level: u8, text: String },
    Bullet(String),
    Paragraph(String),
    CodeBlock { language: String, source: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineKind {
    Plain,
    Bold,
    Code,
    Color,
}

pub(super) fn paint_assistant(
    painter: &mut dyn Painter,
    rect: Rect,
    markdown: &str,
    theme: &ZodeTheme,
) {
    paint_markdown_at(
        painter,
        Point2D::new(rect.origin.x, rect.origin.y + 2.0),
        rect.size.x,
        markdown,
        theme,
    );
}

pub(super) fn paint_user(painter: &mut dyn Painter, rect: Rect, markdown: &str, theme: &ZodeTheme) {
    let bubble = user_bubble_rect(rect, markdown);
    let content_width = (bubble.size.x - USER_HORIZONTAL_PADDING * 2.0).max(1.0);
    painter.fill_round_rect(bubble, 16.0, theme.user_bubble);
    painter.save();
    painter.clip_round_rect(bubble, 16.0);
    paint_markdown_at(
        painter,
        Point2D::new(
            bubble.origin.x + USER_HORIZONTAL_PADDING,
            bubble.origin.y + USER_VERTICAL_PADDING,
        ),
        content_width,
        markdown,
        theme,
    );
    painter.restore();
}

/// The bubble's own rect within an item's full (content-plus-reserved-row)
/// rect. Top-aligned; the hover reveal row occupies the reserved strip below
/// it. Shared by `paint_user` and the transcript module, which right-aligns
/// the reveal row under this same rect so it never drifts from the bubble.
pub(super) fn user_bubble_rect(rect: Rect, markdown: &str) -> Rect {
    let max_width = (rect.size.x * USER_MAX_WIDTH_FRACTION).max(64.0);
    let natural_width =
        natural_markdown_width(markdown) + USER_HORIZONTAL_PADDING * 2.0 + USER_MEASURE_SLOP;
    let bubble_width = natural_width.clamp(64.0, max_width);
    let bubble_height = (rect.size.y - ACTION_ROW_RESERVED).max(USER_MIN_HEIGHT);
    Rect::xywh(
        rect.max_x() - bubble_width,
        rect.origin.y,
        bubble_width,
        bubble_height,
    )
}

pub(super) fn assistant_height(markdown: &str, width: f32) -> f32 {
    assistant_content_height(markdown, width) + ACTION_ROW_RESERVED
}

/// Height of the assistant text block alone, excluding the reserved action
/// row - i.e. where `paint_assistant` actually stops drawing. The transcript
/// module uses this to anchor the action row directly under the text.
pub(super) fn assistant_content_height(markdown: &str, width: f32) -> f32 {
    (markdown_content_height(markdown, width) + 10.0).max(BODY_LINE_HEIGHT + 10.0)
}

pub(super) fn user_height(markdown: &str, width: f32) -> f32 {
    let max_width = (width * USER_MAX_WIDTH_FRACTION).max(64.0);
    let bubble_width =
        (natural_markdown_width(markdown) + USER_HORIZONTAL_PADDING * 2.0 + USER_MEASURE_SLOP)
            .clamp(64.0, max_width);
    user_bubble_height(
        markdown,
        (bubble_width - USER_HORIZONTAL_PADDING * 2.0).max(1.0),
    ) + ACTION_ROW_RESERVED
}

fn user_bubble_height(markdown: &str, width: f32) -> f32 {
    (markdown_content_height(markdown, width) + USER_VERTICAL_PADDING * 2.0).max(USER_MIN_HEIGHT)
}

fn paint_markdown_at(
    painter: &mut dyn Painter,
    origin: Point2D,
    width: f32,
    markdown: &str,
    theme: &ZodeTheme,
) {
    let blocks = conversation_blocks(markdown);
    let mut y = origin.y;
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            y += block_gap(&blocks[index - 1], block);
        }
        match block {
            ConversationBlock::Heading { level, text } => {
                let (size, line_height) = heading_metrics(*level);
                draw_text(
                    painter,
                    text,
                    Point2D::new(origin.x, y),
                    "system-ui",
                    size,
                    600,
                    theme.tokens.foreground,
                );
                y += line_height;
            }
            ConversationBlock::Bullet(source) => {
                let style = TypographyRole::UiBody.style();
                draw_text(
                    painter,
                    "•",
                    Point2D::new(origin.x + 2.0, y),
                    style.family,
                    style.size,
                    600,
                    theme.tokens.foreground,
                );
                let lines = draw_inline(
                    painter,
                    source,
                    Point2D::new(origin.x + BULLET_INDENT, y),
                    (width - BULLET_INDENT).max(1.0),
                    theme,
                );
                y += lines as f32 * BODY_LINE_HEIGHT;
            }
            ConversationBlock::Paragraph(source) => {
                let lines = draw_inline(painter, source, Point2D::new(origin.x, y), width, theme);
                y += lines as f32 * BODY_LINE_HEIGHT;
            }
            ConversationBlock::CodeBlock { language, source } => {
                y += paint_code_block(
                    painter,
                    Rect::xywh(origin.x, y, width, code_block_height(language, source)),
                    language,
                    source,
                    theme,
                );
            }
        }
    }
}

fn paint_code_block(
    painter: &mut dyn Painter,
    rect: Rect,
    language: &str,
    source: &str,
    theme: &ZodeTheme,
) -> f32 {
    painter.fill_round_rect(rect, 12.0, theme.tokens.muted.with_alpha(0.72));
    painter.save();
    painter.clip_round_rect(rect, 12.0);
    let mut y = rect.origin.y + 12.0;
    if !language.is_empty() {
        let caption = TypographyRole::UiCaption.style();
        draw_text(
            painter,
            language,
            Point2D::new(rect.origin.x + 12.0, y),
            caption.family,
            caption.size,
            500,
            theme.tokens.muted_foreground,
        );
        y += caption.line_height + 4.0;
    }
    let code = TypographyRole::Code.style();
    for line in code_lines(source) {
        paint_code_line(
            painter,
            language,
            line,
            Point2D::new(rect.origin.x + 12.0, y),
            &code,
            theme,
        );
        y += code.line_height;
    }
    painter.restore();
    rect.size.y
}

/// Paints one already-wrapped-free code line as a sequence of colored spans
/// (see `code_syntax::tokenize_line`) instead of one plain-colored string.
/// Each span is measured and advanced with the exact same font/size/weight
/// used to draw it, so concatenating the spans lands every glyph at the
/// same x-position a single `draw_text` call would have - highlighting only
/// recolors glyphs, it never changes line height or where the line wraps.
fn paint_code_line(
    painter: &mut dyn Painter,
    language: &str,
    line: &str,
    origin: Point2D,
    style: &crate::TypographyStyle,
    theme: &ZodeTheme,
) {
    let mut x = origin.x;
    for (class, text) in code_syntax::tokenize_line(language, line) {
        let color = code_token_color(class, theme);
        draw_text(
            painter,
            &text,
            Point2D::new(x, origin.y),
            style.family,
            style.size,
            style.weight,
            color,
        );
        x += painter.measure_text_weighted(&text, style.size, style.weight);
    }
}

fn code_token_color(class: TokenClass, theme: &ZodeTheme) -> Color {
    match class {
        TokenClass::Keyword => theme.code_syntax.keyword,
        TokenClass::String => theme.code_syntax.string,
        TokenClass::Comment => theme.code_syntax.comment,
        TokenClass::Number => theme.code_syntax.number,
        TokenClass::Punctuation => theme.code_syntax.punctuation,
        TokenClass::Plain => theme.tokens.foreground,
    }
}

fn markdown_content_height(markdown: &str, width: f32) -> f32 {
    let blocks = conversation_blocks(markdown);
    let mut height = 0.0;
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            height += block_gap(&blocks[index - 1], block);
        }
        height += match block {
            ConversationBlock::Heading { level, .. } => heading_metrics(*level).1,
            ConversationBlock::Bullet(source) => {
                wrapped_inline(&parse_inline(source), (width - BULLET_INDENT).max(1.0)).len() as f32
                    * BODY_LINE_HEIGHT
            }
            ConversationBlock::Paragraph(source) => {
                wrapped_inline(&parse_inline(source), width).len() as f32 * BODY_LINE_HEIGHT
            }
            ConversationBlock::CodeBlock { language, source } => {
                code_block_height(language, source)
            }
        };
    }
    height.max(BODY_LINE_HEIGHT)
}

fn code_block_height(language: &str, source: &str) -> f32 {
    let caption_height = if language.is_empty() {
        0.0
    } else {
        TypographyRole::UiCaption.style().line_height + 4.0
    };
    let line_count = code_lines(source).len().max(1) as f32;
    24.0 + caption_height + line_count * TypographyRole::Code.style().line_height
}

fn code_lines(source: &str) -> Vec<&str> {
    let lines = source.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        vec![""]
    } else {
        lines
    }
}

fn block_gap(previous: &ConversationBlock, current: &ConversationBlock) -> f32 {
    if matches!(previous, ConversationBlock::Bullet(_))
        && matches!(current, ConversationBlock::Bullet(_))
    {
        2.0
    } else {
        BLOCK_GAP
    }
}

fn heading_metrics(level: u8) -> (f32, f32) {
    match level {
        1 => (22.0, 30.0),
        2 => (18.0, 26.0),
        3 => (16.0, 24.0),
        _ => (14.0, BODY_LINE_HEIGHT),
    }
}

fn conversation_blocks(markdown: &str) -> Vec<ConversationBlock> {
    let mut characters = markdown.chars();
    let mut source = characters.by_ref().take(MARKDOWN_LIMIT).collect::<String>();
    if characters.next().is_some() {
        source.push('…');
    }

    let mut blocks = Vec::new();
    let mut prose = String::new();
    let mut code: Option<(String, String)> = None;
    for line in source.lines() {
        if let Some((language, body)) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                blocks.push(ConversationBlock::CodeBlock {
                    language: std::mem::take(language),
                    source: body.trim_end_matches('\n').to_string(),
                });
                code = None;
            } else {
                body.push_str(line);
                body.push('\n');
            }
            continue;
        }

        if let Some(language) = line.trim_start().strip_prefix("```") {
            flush_prose(&mut prose, &mut blocks);
            code = Some((language.trim().to_string(), String::new()));
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    flush_prose(&mut prose, &mut blocks);
    if let Some((language, body)) = code {
        blocks.push(ConversationBlock::CodeBlock {
            language,
            source: body.trim_end_matches('\n').to_string(),
        });
    }
    if blocks.is_empty() {
        blocks.push(ConversationBlock::Paragraph(String::new()));
    }
    blocks
}

fn flush_prose(prose: &mut String, blocks: &mut Vec<ConversationBlock>) {
    if prose.trim().is_empty() {
        prose.clear();
        return;
    }
    for block in parse_blocks(prose, MARKDOWN_LIMIT) {
        blocks.push(match block {
            MdBlock::Heading { level, text } => ConversationBlock::Heading { level, text },
            MdBlock::Bullet(source) => ConversationBlock::Bullet(source),
            MdBlock::Paragraph(source) => heading_from_paragraph(source),
        });
    }
    prose.clear();
}

fn heading_from_paragraph(source: String) -> ConversationBlock {
    for (prefix, level) in [("## ", 2), ("# ", 1)] {
        if let Some(text) = source.strip_prefix(prefix) {
            return ConversationBlock::Heading {
                level,
                text: text.trim().to_string(),
            };
        }
    }
    ConversationBlock::Paragraph(source)
}

fn natural_markdown_width(markdown: &str) -> f32 {
    conversation_blocks(markdown)
        .iter()
        .flat_map(|block| match block {
            ConversationBlock::Heading { text, .. }
            | ConversationBlock::Bullet(text)
            | ConversationBlock::Paragraph(text) => text.lines().collect::<Vec<_>>(),
            ConversationBlock::CodeBlock { source, .. } => source.lines().collect::<Vec<_>>(),
        })
        .map(|line| estimated_text_width(line, InlineKind::Plain))
        .fold(0.0_f32, f32::max)
}

fn draw_inline(
    painter: &mut dyn Painter,
    source: &str,
    origin: Point2D,
    width: f32,
    theme: &ZodeTheme,
) -> usize {
    let lines = wrapped_inline(&parse_inline(source), width);
    for (line_index, line) in lines.iter().enumerate() {
        let mut x = origin.x;
        let y = origin.y + line_index as f32 * BODY_LINE_HEIGHT;
        for run in line {
            let kind = inline_kind(run);
            let content = run.text();
            let run_width = estimated_text_width(content, kind);
            match kind {
                InlineKind::Code => {
                    painter.fill_round_rect(
                        Rect::xywh(x - 3.0, y, run_width + 6.0, 19.0),
                        4.0,
                        theme.tokens.muted,
                    );
                    let code = TypographyRole::Code.style();
                    draw_text(
                        painter,
                        content,
                        Point2D::new(x, y + 1.0),
                        code.family,
                        code.size,
                        500,
                        theme.tokens.foreground,
                    );
                }
                InlineKind::Bold => {
                    let body = TypographyRole::UiBody.style();
                    draw_text(
                        painter,
                        content,
                        Point2D::new(x, y),
                        body.family,
                        body.size,
                        600,
                        theme.tokens.foreground,
                    );
                }
                InlineKind::Color => {
                    let body = TypographyRole::UiBody.style();
                    draw_text(
                        painter,
                        content,
                        Point2D::new(x, y),
                        body.family,
                        body.size,
                        500,
                        theme.zode_purple,
                    );
                }
                InlineKind::Plain => {
                    let body = TypographyRole::UiBody.style();
                    draw_text(
                        painter,
                        content,
                        Point2D::new(x, y),
                        body.family,
                        body.size,
                        body.weight,
                        theme.tokens.foreground,
                    );
                }
            }
            x += run_width;
        }
    }
    lines.len().max(1)
}

fn wrapped_inline(runs: &[MdRun], width: f32) -> Vec<Vec<MdRun>> {
    let max_width = width.max(32.0);
    let mut lines = Vec::<Vec<MdRun>>::new();
    let mut line = Vec::<MdRun>::new();
    let mut line_width = 0.0;

    for run in runs {
        let kind = inline_kind(run);
        for token in inline_tokens(run.text(), kind) {
            let token_width = estimated_text_width(token, kind);
            if token.trim().is_empty() && line.is_empty() {
                continue;
            }
            if token_width <= max_width {
                if !line.is_empty() && line_width + token_width > max_width {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0.0;
                    if token.trim().is_empty() {
                        continue;
                    }
                }
                append_run(&mut line, kind, token);
                line_width += token_width;
                continue;
            }

            for character in token.chars() {
                let mut encoded = [0_u8; 4];
                let character = character.encode_utf8(&mut encoded);
                let character_width = estimated_text_width(character, kind);
                if !line.is_empty() && line_width + character_width > max_width {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0.0;
                }
                append_run(&mut line, kind, character);
                line_width += character_width;
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn inline_tokens(text: &str, kind: InlineKind) -> Vec<&str> {
    if matches!(kind, InlineKind::Code | InlineKind::Color) {
        return vec![text];
    }
    let mut tokens = Vec::new();
    let mut ascii_start = None;
    for (index, character) in text.char_indices() {
        if character.is_ascii() && !character.is_whitespace() {
            ascii_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = ascii_start.take() {
            tokens.push(&text[start..index]);
        }
        tokens.push(&text[index..index + character.len_utf8()]);
    }
    if let Some(start) = ascii_start {
        tokens.push(&text[start..]);
    }
    tokens
}

fn append_run(line: &mut Vec<MdRun>, kind: InlineKind, text: &str) {
    let matching = match (line.last_mut(), kind) {
        (Some(MdRun::Plain(existing)), InlineKind::Plain)
        | (Some(MdRun::Bold(existing)), InlineKind::Bold)
        | (Some(MdRun::Code(existing)), InlineKind::Code)
        | (Some(MdRun::Color(existing)), InlineKind::Color) => Some(existing),
        _ => None,
    };
    if let Some(existing) = matching {
        existing.push_str(text);
        return;
    }
    line.push(match kind {
        InlineKind::Plain => MdRun::Plain(text.to_string()),
        InlineKind::Bold => MdRun::Bold(text.to_string()),
        InlineKind::Code => MdRun::Code(text.to_string()),
        InlineKind::Color => MdRun::Color(text.to_string()),
    });
}

fn inline_kind(run: &MdRun) -> InlineKind {
    match run {
        MdRun::Plain(_) => InlineKind::Plain,
        MdRun::Bold(_) => InlineKind::Bold,
        MdRun::Code(_) => InlineKind::Code,
        MdRun::Color(_) => InlineKind::Color,
    }
}

fn estimated_text_width(text: &str, kind: InlineKind) -> f32 {
    let size = if kind == InlineKind::Code {
        TypographyRole::Code.style().size
    } else {
        TypographyRole::UiBody.style().size
    };
    let weight_factor = if kind == InlineKind::Bold { 1.04 } else { 1.0 };
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                size * 0.34
            } else if !character.is_ascii() {
                size
            } else if "ilI.,:;!'|`".contains(character) {
                size * 0.34
            } else if "mwMW@#%&".contains(character) {
                size * 0.82
            } else {
                size * 0.56
            }
        })
        .sum::<f32>()
        * weight_factor
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    family: &str,
    size: f32,
    weight: u16,
    color: Color,
) {
    let layout = TextLayout::single_run(text, family, size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_code_is_measured_as_a_code_block() {
        let plain = assistant_height("one line", 600.0);
        let code = assistant_height("```rust\nlet answer = 42;\n```", 600.0);

        assert!(code > plain + 24.0);
        assert!(matches!(
            conversation_blocks("```rust\nlet answer = 42;\n```").as_slice(),
            [ConversationBlock::CodeBlock { language, .. }] if language == "rust"
        ));
    }

    #[test]
    fn chinese_text_wraps_without_spaces() {
        let runs = parse_inline("这是一段没有空格但必须在内容列中正确换行的中文文本");
        assert!(wrapped_inline(&runs, 90.0).len() > 2);
    }

    #[test]
    fn short_user_messages_keep_the_reference_compact_height() {
        assert_eq!(
            user_height("继续", 736.0),
            USER_MIN_HEIGHT + ACTION_ROW_RESERVED
        );
    }
}
