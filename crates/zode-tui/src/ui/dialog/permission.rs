//! Permission approval prompt. Shows the pending tool call (with a diff
//! preview for file edits) and its options. Modeled on Claude Code's
//! non-blocking permission UX: the prompt docks inline just above the input
//! (it never covers the conversation), and it does NOT capture the keyboard —
//! the user can keep typing to queue a follow-up message ("插队"). Numbered
//! options (1/2/3) + Esc answer it only while the input is empty, so prose
//! that happens to start with a letter is never swallowed. Multiple requests
//! queue and are shown one at a time by the app.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;
use zode_core::approval::{Approval, ApprovalRequest};

use crate::theme::Theme;
use crate::ui::diff::diff_from_tool_input;

const MIN_POPUP_WIDTH: u16 = 48;
const MAX_POPUP_WIDTH: u16 = 96;
const MAX_POPUP_HEIGHT: u16 = 18;
const MAX_PERMISSION_DIFF_LINES: usize = 8;
/// Hard cap on the inline card height so a huge diff can't swallow the chat.
const MAX_INLINE_HEIGHT: u16 = 14;

/// State for the active approval prompt. Holds the request until the user
/// answers, then responds and reports done. `cwd` resolves the diff
/// preview's path the same way the engine does.
/// The selectable actions, in display + arrow-navigation order.
const OPTIONS: [Approval; 3] = [Approval::AllowOnce, Approval::AllowAlways, Approval::Deny];

pub struct PermissionDialog {
    request: Option<ApprovalRequest>,
    cwd: std::path::PathBuf,
    /// Highlighted action for arrow-key + Enter selection (index into OPTIONS).
    selected: usize,
    /// Screen hitboxes of the answer chips, recorded on render:
    /// `(x_start, x_end, row, approval)` — so a mouse click can answer.
    chip_hits: Vec<(u16, u16, u16, Approval)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRequestIdentity {
    pub(crate) source: Option<String>,
    pub(crate) turn_id: Option<u64>,
    pub(crate) local_op_id: Option<u64>,
}

impl PermissionDialog {
    pub fn new(request: ApprovalRequest, cwd: std::path::PathBuf) -> Self {
        Self {
            request: Some(request),
            cwd,
            selected: 0,
            chip_hits: Vec::new(),
        }
    }

    /// The approval chip under a screen position, if any (mouse click target).
    pub fn approval_at(&self, column: u16, row: u16) -> Option<Approval> {
        self.chip_hits
            .iter()
            .find(|(x0, x1, y, _)| row == *y && column >= *x0 && column < *x1)
            .map(|(_, _, _, a)| *a)
    }

    #[cfg(test)]
    fn test_chip_points(&self) -> Vec<(u16, u16, Approval)> {
        self.chip_hits
            .iter()
            .map(|(x0, _, y, a)| (*x0, *y, *a))
            .collect()
    }

    pub fn tool(&self) -> &str {
        self.request.as_ref().map(|r| r.tool.as_str()).unwrap_or("")
    }

    pub(crate) fn identity(&self) -> Option<PermissionRequestIdentity> {
        self.request
            .as_ref()
            .map(|request| PermissionRequestIdentity {
                source: request.source.clone(),
                turn_id: request.turn_id,
                local_op_id: request.local_op_id,
            })
    }

    pub(crate) fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// Remove and return the still-pending request. Lifecycle cleanup and the
    /// main answer path use this so sender failure and typed ownership can be
    /// handled explicitly by the app rather than hidden inside the dialog.
    pub(crate) fn take_request(&mut self) -> Option<ApprovalRequest> {
        self.chip_hits.clear();
        self.request.take()
    }

    /// Move the highlight to the previous action (↑/←), wrapping.
    pub fn select_prev(&mut self) {
        self.selected = self.selected.checked_sub(1).unwrap_or(OPTIONS.len() - 1);
    }

    /// Move the highlight to the next action (↓/→), wrapping.
    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % OPTIONS.len();
    }

    /// The currently highlighted action (what Enter confirms).
    pub fn selected_approval(&self) -> Approval {
        OPTIONS[self.selected]
    }

    /// Respond with `approval` and report done. Returns true if a pending
    /// request was answered (false if it had already been taken).
    pub fn answer(&mut self, approval: Approval) -> bool {
        self.take_request()
            .is_some_and(|request| request.respond(approval).is_ok())
    }

    /// The card's body lines (summary + optional diff preview + options hint).
    /// All card lines (body + footer), used only for height/width estimation.
    fn lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = self.body_lines(theme);
        lines.extend(self.footer_lines(theme).0);
        lines
    }

    /// The scrollable/clippable upper part: what the tool wants to do and a
    /// diff preview. May be long (e.g. a big file write) — it gets truncated
    /// so the footer (the action keys) is never pushed off the card.
    fn body_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let Some(req) = &self.request else {
            return Vec::new();
        };
        let mut lines = vec![Line::from(req.summary()), Line::from("")];
        if let Some(diff) = diff_from_tool_input(&req.input, &self.cwd, theme) {
            let truncated = diff.len() > MAX_PERMISSION_DIFF_LINES;
            lines.extend(diff.into_iter().take(MAX_PERMISSION_DIFF_LINES));
            if truncated {
                lines.push(Line::styled(
                    format!("… {}", crate::tr("diff preview truncated")),
                    Style::default().fg(theme.fg_subtle),
                ));
            }
        }
        lines
    }

    /// The PINNED footer: the answer keys (always row 0 so they survive a tight
    /// card) plus the non-blocking hint (row 1, dropped first if space is short).
    /// Also returns each chip's `(x_start, x_end)` RELATIVE to the line start,
    /// paired with its approval — render_card turns these into screen hitboxes.
    fn footer_lines(&self, theme: &Theme) -> (Vec<Line<'static>>, Vec<(u16, u16, Approval)>) {
        // Each action is a chip; the arrow-key/Enter highlight marks the active
        // one. The leading digit keeps the 1/2/3 direct-pick affordance.
        let labels = [
            format!("1 {}", crate::tr("allow once")),
            format!("2 {}", crate::tr("allow always")),
            format!("3 {}", crate::tr("deny")),
        ];
        let normal = Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD);
        let active = Style::default()
            .fg(theme.bg_primary)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD);
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut chips = Vec::new();
        let mut x = 0u16;
        for (i, lbl) in labels.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
                x += 2;
            }
            let style = if i == self.selected { active } else { normal };
            let chip = format!(" {lbl} ");
            let w = UnicodeWidthStr::width(chip.as_str()) as u16;
            chips.push((x, x + w, OPTIONS[i]));
            x += w;
            spans.push(Span::styled(chip, style));
        }
        spans.push(Span::styled(
            format!("   ↑↓/1-3 · enter · esc {}", crate::tr("deny")),
            Style::default().fg(theme.fg_subtle),
        ));
        let lines = vec![
            Line::from(spans),
            Line::styled(
                crate::tr("…or just keep typing to queue a message — this prompt won't block you."),
                Style::default().fg(theme.fg_subtle),
            ),
        ];
        (lines, chips)
    }

    /// Rows the inline card wants, given the available `width` (accounts for a
    /// long summary wrapping). Clamped so it never dominates the chat area.
    pub fn desired_height(&self, width: u16, theme: &Theme) -> u16 {
        let inner = width.saturating_sub(2).max(1) as usize;
        let body: u16 = self
            .lines(theme)
            .iter()
            .map(|l| {
                let w = line_width(l);
                // How many rows this logical line occupies once wrapped.
                w.max(1).div_ceil(inner) as u16
            })
            .sum();
        body.saturating_add(2).clamp(5, MAX_INLINE_HEIGHT)
    }

    /// Render the prompt as an inline card filling `area` (docked above the
    /// input). Does not center; does not cover the conversation.
    pub fn render_inline(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        if self.request.is_none() || area.height == 0 {
            return;
        }
        self.render_card(f, area, theme);
    }

    /// Fallback centered popup, used only when the terminal is too short to
    /// dock the card inline above the input.
    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        if self.request.is_none() {
            return;
        }
        let popup = permission_popup_area(area, &self.lines(theme));
        self.render_card(f, popup, theme);
    }

    /// Draw the bordered card into `area`, with the body (summary + diff) on
    /// top and the answer-key footer PINNED to the bottom rows so a long file
    /// write can never push the `[1] [2] [3]` keys off the card. The body
    /// clips/truncates instead.
    fn render_card(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.chip_hits.clear();
        f.render_widget(Clear, area);
        let block = Block::default()
            .title(format!(" {}: {} ", crate::tr("Permission"), self.tool()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text));
        let inner = block.inner(area);
        f.render_widget(block, area);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Reserve the bottom rows for the footer (≤2; at least the keys row).
        let (footer, chips) = self.footer_lines(theme);
        let footer_h = (footer.len() as u16).min(inner.height);
        let body_h = inner.height.saturating_sub(footer_h);
        // The keys row is the footer's first line; anchor the chip hitboxes to
        // its screen position so a click can answer.
        let keys_row = inner.y.saturating_add(body_h);
        self.chip_hits = chips
            .into_iter()
            .filter(|(_, x1, _)| inner.x + x1 <= inner.x + inner.width)
            .map(|(x0, x1, a)| (inner.x + x0, inner.x + x1, keys_row, a))
            .collect();

        if body_h > 0 {
            let body_area = Rect::new(inner.x, inner.y, inner.width, body_h);
            f.render_widget(
                Paragraph::new(self.body_lines(theme))
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: false }),
                body_area,
            );
        }
        let footer_area = Rect::new(
            inner.x,
            inner.y.saturating_add(body_h),
            inner.width,
            footer_h,
        );
        f.render_widget(
            Paragraph::new(footer)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            footer_area,
        );
    }
}

fn permission_popup_area(area: Rect, lines: &[Line<'_>]) -> Rect {
    if area.width <= 4 || area.height <= 4 {
        return area;
    }
    let max_width = area.width.saturating_sub(4).clamp(1, MAX_POPUP_WIDTH);
    let min_width = MIN_POPUP_WIDTH.min(max_width);
    let content_width = lines.iter().map(line_width).max().unwrap_or(0);
    let width = (content_width as u16)
        .saturating_add(4)
        .clamp(min_width, max_width);

    let max_height = area.height.saturating_sub(4).clamp(1, MAX_POPUP_HEIGHT);
    let height = (lines.len() as u16).saturating_add(2).clamp(3, max_height);

    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn line_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

/// Map a numeric option key (`1`/`2`/`3`) to its approval. Letters are NOT
/// mapped: while a permission is pending the input box stays live, so prose
/// keystrokes (incl. ones starting "y"/"a"/"n") must reach the input untouched.
pub(crate) fn approval_for_key(c: char) -> Option<Approval> {
    match c {
        '1' => Some(Approval::AllowOnce),
        '2' => Some(Approval::AllowAlways),
        '3' => Some(Approval::Deny),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, Terminal};
    use zode_core::approval::approval_queue;

    #[tokio::test]
    async fn answer_keys_stay_visible_with_huge_params() {
        // A file write with a very long parameter must NOT push the [1][2][3]
        // keys off the card — they are pinned to the bottom (user report:
        // "参数内容太多了，把权限的 1 2 3 挤掉").
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        let huge = "x".repeat(8000);
        tokio::spawn(async move {
            q.request("Bash", &serde_json::json!({ "command": huge }), None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        let theme = ThemeStore::with_builtins().resolve(None);
        // A deliberately short card so a long body would otherwise overflow it.
        let backend = TestBackend::new(70, 8);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| dialog.render_inline(f, f.area(), &theme))
            .unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            content.contains("1 allow once"),
            "answer keys must stay visible even with huge params; got:\n{content}"
        );
        drop(queue);
    }

    #[tokio::test]
    async fn clicking_an_answer_chip_maps_to_its_approval() {
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut term = Terminal::new(TestBackend::new(70, 10)).unwrap();
        term.draw(|f| dialog.render_inline(f, f.area(), &theme))
            .unwrap();
        // One recorded hitbox per chip, in display order.
        let chips = dialog.test_chip_points();
        assert_eq!(chips.len(), 3);
        assert_eq!(
            chips.iter().map(|c| c.2).collect::<Vec<_>>(),
            vec![Approval::AllowOnce, Approval::AllowAlways, Approval::Deny]
        );
        // A click inside a chip resolves to its approval; elsewhere to None.
        let (col, row, _) = chips[1];
        assert_eq!(dialog.approval_at(col, row), Some(Approval::AllowAlways));
        assert_eq!(dialog.approval_at(col, row + 3), None);
        assert_eq!(dialog.approval_at(0, row), None);
        drop(queue);
    }

    #[test]
    fn key_maps_to_approval() {
        assert_eq!(approval_for_key('1'), Some(Approval::AllowOnce));
        assert_eq!(approval_for_key('2'), Some(Approval::AllowAlways));
        assert_eq!(approval_for_key('3'), Some(Approval::Deny));
        // Letters must NOT answer — they belong to the live input box.
        assert_eq!(approval_for_key('y'), None);
        assert_eq!(approval_for_key('a'), None);
        assert_eq!(approval_for_key('n'), None);
    }

    #[tokio::test]
    async fn answer_responds_and_reports_done() {
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        let join =
            tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        assert!(dialog.answer(Approval::AllowOnce)); // responded
        assert!(!dialog.answer(Approval::AllowOnce)); // request already taken
        assert_eq!(join.await.unwrap(), Approval::AllowOnce);
    }

    #[tokio::test]
    async fn answer_reports_failure_when_requester_has_gone_away() {
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        let join =
            tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        join.abort();
        assert!(join.await.is_err());

        assert!(!dialog.answer(Approval::AllowAlways));
        assert!(dialog.take_request().is_none());
    }

    #[tokio::test]
    async fn dialog_exposes_typed_request_identity_before_take() {
        let (queue, mut rx) = approval_queue();
        queue.bind_local_operation("tab-4", 12);
        let q = queue.clone();
        let join = tokio::spawn(async move {
            q.request("Bash", &serde_json::json!({}), Some("tab-4".to_string()))
                .await
        });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());

        let identity = dialog.identity().expect("request remains pending");
        assert_eq!(identity.source.as_deref(), Some("tab-4"));
        assert_eq!(identity.turn_id, None);
        assert_eq!(identity.local_op_id, Some(12));
        let req = dialog.take_request().expect("request can be taken once");
        assert!(dialog.identity().is_none());
        assert!(dialog.take_request().is_none());
        req.respond(Approval::Deny).unwrap();
        assert_eq!(join.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn arrow_selection_wraps_and_confirms_the_highlighted_action() {
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        let join =
            tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        // Default highlight is the first action.
        assert_eq!(dialog.selected_approval(), Approval::AllowOnce);
        dialog.select_next(); // → allow always
        dialog.select_next(); // → deny
        assert_eq!(dialog.selected_approval(), Approval::Deny);
        dialog.select_next(); // wraps → allow once
        assert_eq!(dialog.selected_approval(), Approval::AllowOnce);
        dialog.select_prev(); // wraps back → deny
        assert_eq!(dialog.selected_approval(), Approval::Deny);
        // Enter would answer with the highlighted action.
        let pick = dialog.selected_approval();
        assert!(dialog.answer(pick));
        assert_eq!(join.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn inline_height_is_bounded() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(None);
        let (queue, mut rx) = approval_queue();
        let q = queue.clone();
        tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let dialog = PermissionDialog::new(req, std::env::temp_dir());
        let h = dialog.desired_height(80, &theme);
        assert!(
            (5..=MAX_INLINE_HEIGHT).contains(&h),
            "height {h} out of range"
        );
    }

    #[test]
    fn popup_area_is_compact_on_wide_terminals() {
        let lines = vec![
            Line::from("FileWrite /Users/kayshen/Workspace/ZSeven-W/zode/target/debug/hello.py"),
            Line::from(""),
            Line::from("+print(\"Hello, World!\")"),
            Line::from(""),
            Line::from("[1] allow once   [2] allow always   [3] deny   (Esc deny)"),
        ];

        let popup = permission_popup_area(Rect::new(0, 0, 220, 70), &lines);

        assert!(
            popup.width <= 96,
            "popup width should be compact: {popup:?}"
        );
        assert!(
            popup.height <= 12,
            "popup height should be compact: {popup:?}"
        );
        assert!(popup.width >= 48, "popup should remain readable: {popup:?}");
    }

    #[test]
    fn popup_area_stays_inside_tight_terminals() {
        let lines = vec![Line::from("[1] allow once   [2] allow always   [3] deny")];

        let popup = permission_popup_area(Rect::new(0, 0, 52, 12), &lines);

        assert!(popup.x > 0);
        assert!(popup.y > 0);
        assert!(popup.x + popup.width <= 52);
        assert!(popup.y + popup.height <= 12);
    }
}
