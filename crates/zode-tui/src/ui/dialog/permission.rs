//! Permission approval dialog. Shows the pending tool call (with a diff
//! preview for file edits) and collects y/a/n. Multiple requests queue and
//! are shown one at a time by the app.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use zode_core::approval::{Approval, ApprovalRequest};

use crate::theme::Theme;
use crate::ui::centered;
use crate::ui::diff::{diff_from_tool_input, MAX_DIFF_LINES};

/// State for the active approval prompt. Holds the request until the user
/// answers, then responds and reports done.
pub struct PermissionDialog {
    request: Option<ApprovalRequest>,
}

impl PermissionDialog {
    pub fn new(request: ApprovalRequest) -> Self {
        Self {
            request: Some(request),
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
        if let Some(diff) = diff_from_tool_input(&req.input, theme) {
            lines.extend(diff.into_iter().take(MAX_DIFF_LINES));
            lines.push(Line::from(""));
        }
        lines.push(Line::styled(
            "[y] allow once   [a] allow always   [N] deny",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));

        let popup = centered(area, 70, 60);
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
        let join = tokio::spawn(async move { q.request("Bash", &serde_json::json!({})).await });
        let req = rx.next().await.unwrap();
        let mut dialog = PermissionDialog::new(req);
        assert!(!dialog.on_key('x')); // not a decision key
        assert!(dialog.on_key('y')); // responded
        assert_eq!(join.await.unwrap(), Approval::AllowOnce);
    }
}
