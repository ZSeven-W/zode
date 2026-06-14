//! Session picker (/sessions, /resume). Lists SessionMeta newest-first with a
//! substring title filter. Enter resumes the selected session in a new tab;
//! Delete removes the session file. Navigation/visibility live here; the app
//! owns the resume/delete side effects.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use zode_core::session_meta::SessionMeta;

use crate::theme::Theme;
use crate::ui::centered;

pub struct SessionPicker {
    all: Vec<SessionMeta>,
    filter: String,
    state: ListState,
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
        }
    }

    pub fn set_filter(&mut self, f: &str) {
        self.filter = f.to_lowercase();
        self.state.select(Some(0));
    }

    pub fn push_filter_char(&mut self, c: char) {
        for lc in c.to_lowercase() {
            self.filter.push(lc);
        }
        self.state.select(Some(0));
    }

    pub fn pop_filter_char(&mut self) {
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
        let len = self.visible().len().max(1);
        let i = (self.state.selected().unwrap_or(0) + 1) % len;
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
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
        let len = self.visible().len();
        if len == 0 {
            self.state.select(Some(0));
            return;
        }
        let selected = self.state.selected().unwrap_or(0).min(len - 1);
        self.state.select(Some(selected.saturating_sub(rows)));
    }

    /// Drop the given session id from the in-memory list (after the app has
    /// deleted the file), keeping the selection in range.
    pub fn remove(&mut self, id: &str) {
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
        let title = if self.filter.is_empty() {
            " Sessions  [Enter] resume  [Del] delete  [Esc] close ".to_string()
        } else {
            format!(" Sessions  /{} ", self.filter)
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.accent)
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
    fn wheel_scroll_clamps_without_wrapping() {
        let mut p = SessionPicker::new(metas());

        p.scroll_down(10);
        assert_eq!(p.selected().unwrap().id, "bbbbbbbb2");

        p.scroll_up(10);
        assert_eq!(p.selected().unwrap().id, "aaaaaaaa1");
    }
}
