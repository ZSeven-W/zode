//! Geometry + text-wrapping helpers for the `AskUserQuestion` modal.
//!
//! Kept out of `question.rs` so the dialog file stays about behaviour: these
//! are pure functions over widths and strings, and are unit-tested here.

use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Popup width the dialog aims for when its content is short — the historic
/// fixed width, kept so simple questions look exactly as they used to.
const PREFERRED_WIDTH: u16 = 76;

/// Split `text` into visual lines at most `width` columns wide.
///
/// Widths are unicode-aware (a CJK glyph costs 2 columns) and breaks land on
/// character boundaries, so a grapheme is never cut in half. A space in the
/// pending line is preferred as the break point; CJK text has none, so it
/// falls back to breaking mid-run. Always returns at least one line, so an
/// empty string still renders as a blank row.
pub(super) fn wrap_text(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    // Where the last space run started: `(byte index, column)` in `cur`, plus
    // the byte index of the word that follows it once we've seen one.
    let mut space_at: Option<(usize, usize)> = None;
    let mut brk: Option<(usize, usize, usize)> = None;

    for ch in text.chars() {
        // Control characters would desync the width math against what the
        // terminal actually paints; tabs become a single space.
        let ch = match ch {
            '\n' => {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
                space_at = None;
                brk = None;
                continue;
            }
            '\t' => ' ',
            c if c.is_control() => continue,
            c => c,
        };
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cur_w + cw > width && !cur.is_empty() {
            match brk {
                // Break after the last complete word, carrying the tail over.
                Some((head_end, _, tail_start)) if head_end > 0 => {
                    let tail = cur[tail_start..].to_string();
                    cur.truncate(head_end);
                    out.push(std::mem::take(&mut cur));
                    cur = tail;
                    cur_w = UnicodeWidthStr::width(cur.as_str());
                }
                _ => {
                    out.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
            }
            space_at = None;
            brk = None;
        }
        if ch == ' ' {
            if space_at.is_none() && !cur.is_empty() {
                space_at = Some((cur.len(), cur_w));
            }
        } else if let Some((idx, w)) = space_at.take() {
            brk = Some((idx, w, cur.len()));
        }
        cur.push(ch);
        cur_w += cw;
    }
    out.push(cur);
    out
}

/// Columns `text` occupies on screen.
pub(super) fn text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}

/// Popup width for the given natural content width: never below 40 (or the
/// terminal, when it is narrower), at least the historic 76 when there is
/// room, and free to grow to 90% of the terminal for long content.
pub(super) fn modal_width(area: Rect, natural: u16) -> u16 {
    let max_w = area.width.saturating_sub(6);
    let grow_cap = max_w.min((area.width as u32 * 9 / 10) as u16);
    let target = natural
        .max(PREFERRED_WIDTH)
        .min(grow_cap.max(PREFERRED_WIDTH));
    max_w.min(target).max(max_w.min(40))
}

/// Popup height for the wanted line count, clamped to the terminal.
pub(super) fn modal_height(area: Rect, want: u16) -> u16 {
    let max_h = area.height.saturating_sub(4);
    max_h.min(want).max(max_h.min(8))
}

/// Center a `width` × `height` popup inside `area`.
pub(super) fn modal_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// First visible body line, keeping the cursor line inside the window.
pub(super) fn scroll_start(cursor_row: usize, total: usize, height: usize) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    let max_start = total - height;
    // Keep the cursor row within the window, biased so it isn't on the last line.
    cursor_row.saturating_sub(height / 2).min(max_start)
}

/// Horizontal scroll for the tab strip so the active chip stays visible.
/// `chips` holds `(x_start, x_end, tab)` relative to the strip's own start.
pub(super) fn strip_scroll(chips: &[(u16, u16, usize)], active: usize, width: u16) -> u16 {
    let total = chips.last().map(|&(_, x1, _)| x1).unwrap_or(0);
    if width == 0 || total <= width {
        return 0;
    }
    let Some(&(x0, x1, _)) = chips.iter().find(|&&(_, _, tab)| tab == active) else {
        return 0;
    };
    // Enough to bring the chip's right edge into view, but never past its left.
    x1.saturating_sub(width).min(x0).min(total - width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_keeps_every_line_within_the_width() {
        let cjk = "请选择一个你希望采用的实现方案，我们会据此继续推进后续的开发工作";
        let lines = wrap_text(cjk, 20);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(text_width(l) <= 20, "line too wide: {l}");
        }
        // Nothing is dropped on the way.
        assert_eq!(lines.concat(), cjk);
    }

    #[test]
    fn wrap_prefers_spaces_when_the_text_has_them() {
        let lines = wrap_text("alpha beta gamma delta", 12);
        // The space at the break is dropped rather than dangling at the edge.
        assert_eq!(lines, vec!["alpha beta", "gamma delta"]);
    }

    #[test]
    fn wrap_falls_back_to_character_breaks() {
        let lines = wrap_text("aaaaaaaa", 3);
        assert_eq!(lines, vec!["aaa", "aaa", "aa"]);
    }

    #[test]
    fn wrap_returns_one_line_for_empty_text() {
        assert_eq!(wrap_text("", 20), vec![String::new()]);
    }

    #[test]
    fn wrap_breaks_on_newlines() {
        assert_eq!(wrap_text("a\nb", 20), vec!["a", "b"]);
    }

    #[test]
    fn modal_width_keeps_the_classic_size_for_short_content() {
        let area = Rect::new(0, 0, 120, 40);
        assert_eq!(modal_width(area, 30), 76);
    }

    #[test]
    fn modal_width_grows_for_long_content_but_stays_inside_the_terminal() {
        let area = Rect::new(0, 0, 120, 40);
        let w = modal_width(area, 200);
        assert!(w > 76);
        assert!(w <= area.width - 6);
        assert!(w <= area.width * 9 / 10);
    }

    #[test]
    fn modal_width_fits_a_narrow_terminal() {
        let area = Rect::new(0, 0, 30, 20);
        let w = modal_width(area, 200);
        assert!(w <= 30);
    }

    #[test]
    fn strip_scroll_reveals_the_active_chip() {
        let chips = vec![(1, 11, 0), (12, 22, 1), (23, 33, 2)];
        assert_eq!(strip_scroll(&chips, 0, 40), 0); // fits, no scroll
        assert_eq!(strip_scroll(&chips, 0, 20), 0); // active already visible
        assert_eq!(strip_scroll(&chips, 2, 20), 13); // 33 - 20
    }
}
