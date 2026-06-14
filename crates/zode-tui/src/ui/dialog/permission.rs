//! Permission approval dialog. Shows the pending tool call (with a diff
//! preview for file edits) and collects y/a/n. Multiple requests queue and
//! are shown one at a time by the app.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
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

/// State for the active approval prompt. Holds the request until the user
/// answers, then responds and reports done. `cwd` resolves the diff
/// preview's path the same way the engine does.
pub struct PermissionDialog {
    request: Option<ApprovalRequest>,
    cwd: std::path::PathBuf,
}

impl PermissionDialog {
    pub fn new(request: ApprovalRequest, cwd: std::path::PathBuf) -> Self {
        Self {
            request: Some(request),
            cwd,
        }
    }

    pub fn tool(&self) -> &str {
        self.request.as_ref().map(|r| r.tool.as_str()).unwrap_or("")
    }

    /// Handle a key; returns true if the dialog responded and is done.
    pub fn on_key(&mut self, c: char) -> bool {
        if let Some(approval) = approval_for_key(c) {
            if let Some(req) = self.request.take() {
                let _ = req.respond(approval);
            }
            return true;
        }
        false
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let Some(req) = &self.request else {
            return;
        };
        let mut lines = vec![Line::from(req.summary()), Line::from("")];
        if let Some(diff) = diff_from_tool_input(&req.input, &self.cwd, theme) {
            let truncated = diff.len() > MAX_PERMISSION_DIFF_LINES;
            lines.extend(diff.into_iter().take(MAX_PERMISSION_DIFF_LINES));
            if truncated {
                lines.push(Line::styled(
                    "… diff preview truncated",
                    Style::default().fg(theme.fg_subtle),
                ));
            }
            lines.push(Line::from(""));
        }
        lines.push(Line::styled(
            "[y] allow once   [a] allow always   [N] deny",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));

        let popup = permission_popup_area(area, &lines);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .title(format!(" Permission: {} ", req.tool))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text));
        let para = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false });
        f.render_widget(para, popup);
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

pub(crate) fn approval_for_key(c: char) -> Option<Approval> {
    match c.to_ascii_lowercase() {
        'y' => Some(Approval::AllowOnce),
        'a' => Some(Approval::AllowAlways),
        'n' => Some(Approval::Deny),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::approval::approval_queue;

    #[test]
    fn key_maps_to_approval() {
        assert_eq!(approval_for_key('y'), Some(Approval::AllowOnce));
        assert_eq!(approval_for_key('a'), Some(Approval::AllowAlways));
        assert_eq!(approval_for_key('n'), Some(Approval::Deny));
        assert_eq!(approval_for_key('x'), None);
    }

    #[tokio::test]
    async fn on_key_responds_and_reports_done() {
        let (queue, mut rx) = approval_queue();
        // Tool side requests; we capture the request to feed the dialog.
        let q = queue.clone();
        let join =
            tokio::spawn(async move { q.request("Bash", &serde_json::json!({}), None).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req, std::env::temp_dir());
        assert!(!dialog.on_key('x')); // not a decision key
        assert!(dialog.on_key('y')); // responded
        assert_eq!(join.await.unwrap(), Approval::AllowOnce);
    }

    #[test]
    fn popup_area_is_compact_on_wide_terminals() {
        let lines = vec![
            Line::from("FileWrite /Users/kayshen/Workspace/ZSeven-W/zode/target/debug/hello.py"),
            Line::from(""),
            Line::from("+print(\"Hello, World!\")"),
            Line::from(""),
            Line::from("[y] allow once   [a] allow always   [N] deny"),
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
        let lines = vec![Line::from("[y] allow once   [a] allow always   [N] deny")];

        let popup = permission_popup_area(Rect::new(0, 0, 52, 12), &lines);

        assert!(popup.x > 0);
        assert!(popup.y > 0);
        assert!(popup.x + popup.width <= 52);
        assert!(popup.y + popup.height <= 12);
    }
}
