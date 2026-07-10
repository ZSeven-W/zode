//! Session picker (/sessions, /resume). Lists SessionMeta newest-first with a
//! substring title filter. Enter resumes the selected session in a new tab;
//! Delete removes the session file. Navigation/visibility live here; the app
//! owns the resume/delete side effects.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use zode_core::session_meta::SessionMeta;

use crate::theme::Theme;
use crate::ui::centered;

/// Outcome of a Delete keypress: the first press only arms the deletion,
/// a second press on the same selection confirms it.
#[derive(Debug)]
pub enum DeletePress {
    Armed,
    Confirmed(SessionMeta),
}

pub struct SessionPicker {
    all: Vec<SessionMeta>,
    filter: String,
    state: ListState,
    /// Session id armed for deletion; cleared by any navigation/filter change.
    pending_delete: Option<String>,
}

impl SessionPicker {
    pub fn new(mut metas: Vec<SessionMeta>) -> Self {
        metas.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            all: metas,
            filter: String::new(),
            state,
            pending_delete: None,
        }
    }

    pub fn set_filter(&mut self, f: &str) {
        self.pending_delete = None;
        self.filter = f.to_lowercase();
        self.state.select(Some(0));
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.pending_delete = None;
        for lc in c.to_lowercase() {
            self.filter.push(lc);
        }
        self.state.select(Some(0));
    }

    pub fn pop_filter_char(&mut self) {
        self.pending_delete = None;
        self.filter.pop();
        self.state.select(Some(0));
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn visible(&self) -> Vec<&SessionMeta> {
        self.all
            .iter()
            .filter(|m| self.filter.is_empty() || m.title.to_lowercase().contains(&self.filter))
            .collect()
    }

    pub fn selected(&self) -> Option<SessionMeta> {
        self.visible()
            .get(self.state.selected().unwrap_or(0))
            .map(|m| (*m).clone())
    }

    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    pub fn next(&mut self) {
        self.pending_delete = None;
        let len = self.visible().len().max(1);
        let i = (self.state.selected().unwrap_or(0) + 1) % len;
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        self.pending_delete = None;
        let len = self.visible().len().max(1);
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .checked_sub(1)
            .unwrap_or(len - 1);
        self.state.select(Some(i));
    }

    pub fn scroll_down(&mut self, rows: usize) {
        self.pending_delete = None;
        let len = self.visible().len();
        if len == 0 {
            self.state.select(Some(0));
            return;
        }
        let selected = self.state.selected().unwrap_or(0);
        self.state
            .select(Some(selected.saturating_add(rows).min(len - 1)));
    }

    pub fn scroll_up(&mut self, rows: usize) {
        self.pending_delete = None;
        let len = self.visible().len();
        if len == 0 {
            self.state.select(Some(0));
            return;
        }
        let selected = self.state.selected().unwrap_or(0).min(len - 1);
        self.state.select(Some(selected.saturating_sub(rows)));
    }

    /// Handle the Delete key. Deleting a transcript is irreversible, so the
    /// first press only arms it: the same selection must be Delete'd again to
    /// confirm. Returns `None` when nothing is selected.
    pub fn press_delete(&mut self) -> Option<DeletePress> {
        let meta = self.selected()?;
        if self.pending_delete.as_deref() == Some(meta.id.as_str()) {
            self.pending_delete = None;
            return Some(DeletePress::Confirmed(meta));
        }
        self.pending_delete = Some(meta.id);
        Some(DeletePress::Armed)
    }

    /// Disarm a pending delete (Esc). Returns whether one was armed — the
    /// caller keeps the picker open in that case instead of closing it.
    pub fn cancel_pending_delete(&mut self) -> bool {
        self.pending_delete.take().is_some()
    }

    pub fn pending_delete_id(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    /// Drop the given session id from the in-memory list (after the app has
    /// deleted the file), keeping the selection in range.
    pub fn remove(&mut self, id: &str) {
        if self.pending_delete.as_deref() == Some(id) {
            self.pending_delete = None;
        }
        self.all.retain(|m| m.id != id);
        let len = self.visible().len();
        if let Some(sel) = self.state.selected() {
            if len == 0 {
                self.state.select(Some(0));
            } else if sel >= len {
                self.state.select(Some(len - 1));
            }
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let items: Vec<ListItem> = self
            .visible()
            .iter()
            .map(|m| {
                let short: String = m.id.chars().take(8).collect();
                ListItem::new(Line::from(format!("{}  ({short}, {})", m.title, m.model)))
            })
            .collect();
        let popup = centered(area, 70, 60);
        f.render_widget(Clear, popup);
        // An armed delete repurposes the title as the confirmation prompt and
        // turns the selection red until it's confirmed or cancelled.
        let armed = self
            .pending_delete
            .as_deref()
            .is_some_and(|id| self.selected().is_some_and(|m| m.id == id));
        let title = if armed {
            format!(
                " {}  [Del] {}  [Esc] {} ",
                crate::tr("Sessions"),
                crate::tr("confirm delete"),
                crate::tr("cancel")
            )
        } else if self.filter.is_empty() {
            format!(
                " {}  [Enter] {}  [Del] {}  [Esc] {} ",
                crate::tr("Sessions"),
                crate::tr("resume"),
                crate::tr("delete"),
                crate::tr("close")
            )
        } else {
            format!(" {}  /{} ", crate::tr("Sessions"), self.filter)
        };
        let border = if armed { Color::Red } else { theme.accent };
        let highlight_bg = if armed { Color::Red } else { theme.accent };
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            )
            .highlight_style(
                Style::default()
                    .bg(highlight_bg)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, popup, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metas() -> Vec<SessionMeta> {
        vec![
            SessionMeta {
                id: "aaaaaaaa1".into(),
                title: "fix bug".into(),
                cwd: "/p".into(),
                model: "m".into(),
                updated_at: 200,
            },
            SessionMeta {
                id: "bbbbbbbb2".into(),
                title: "add feature".into(),
                cwd: "/p".into(),
                model: "m".into(),
                updated_at: 100,
            },
        ]
    }

    #[test]
    fn filter_matches_title() {
        let mut p = SessionPicker::new(metas());
        p.set_filter("feat");
        assert_eq!(p.visible().len(), 1);
        assert_eq!(p.visible()[0].id, "bbbbbbbb2");
    }

    #[test]
    fn selected_returns_meta_newest_first() {
        let mut p = SessionPicker::new(metas());
        assert_eq!(p.selected().unwrap().id, "aaaaaaaa1"); // newest first
        p.next();
        assert_eq!(p.selected().unwrap().id, "bbbbbbbb2");
        p.next(); // wraps
        assert_eq!(p.selected().unwrap().id, "aaaaaaaa1");
    }

    #[test]
    fn remove_drops_session_and_clamps_selection() {
        let mut p = SessionPicker::new(metas());
        p.next(); // select bbbb (index 1)
        p.remove("bbbbbbbb2");
        assert_eq!(p.visible().len(), 1);
        assert_eq!(p.selected().unwrap().id, "aaaaaaaa1");
    }

    #[test]
    fn delete_requires_second_press_to_confirm() {
        let mut p = SessionPicker::new(metas());
        // First press only arms the pending delete.
        assert!(matches!(p.press_delete(), Some(DeletePress::Armed)));
        assert_eq!(p.pending_delete_id(), Some("aaaaaaaa1"));
        // Second press on the same selection confirms.
        match p.press_delete() {
            Some(DeletePress::Confirmed(meta)) => assert_eq!(meta.id, "aaaaaaaa1"),
            other => panic!("expected Confirmed, got {other:?}"),
        }
        assert_eq!(p.pending_delete_id(), None);
    }

    #[test]
    fn navigation_and_filter_cancel_pending_delete() {
        let mut p = SessionPicker::new(metas());
        assert!(matches!(p.press_delete(), Some(DeletePress::Armed)));
        p.next(); // moving the selection disarms
        assert_eq!(p.pending_delete_id(), None);
        // The next press re-arms (on the new selection) instead of deleting.
        assert!(matches!(p.press_delete(), Some(DeletePress::Armed)));
        assert_eq!(p.pending_delete_id(), Some("bbbbbbbb2"));
        p.push_filter_char('f'); // filtering disarms too
        assert_eq!(p.pending_delete_id(), None);
    }

    #[test]
    fn esc_cancels_pending_delete_before_closing() {
        let mut p = SessionPicker::new(metas());
        assert!(!p.cancel_pending_delete()); // nothing armed yet
        assert!(matches!(p.press_delete(), Some(DeletePress::Armed)));
        assert!(p.cancel_pending_delete()); // armed → cancelled, picker stays
        assert_eq!(p.pending_delete_id(), None);
        assert!(matches!(p.press_delete(), Some(DeletePress::Armed)));
    }

    #[test]
    fn wheel_scroll_clamps_without_wrapping() {
        let mut p = SessionPicker::new(metas());

        p.scroll_down(10);
        assert_eq!(p.selected().unwrap().id, "bbbbbbbb2");

        p.scroll_up(10);
        assert_eq!(p.selected().unwrap().id, "aaaaaaaa1");
    }
}
