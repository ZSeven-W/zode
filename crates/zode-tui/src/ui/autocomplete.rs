//! Slash-command autocomplete popup. Activates when the input line starts
//! with "/", filters via CommandRegistry::lookup, navigable with ↑↓/Tab,
//! confirmed with Enter/Tab, dismissed with Esc.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use zode_core::commands::{CommandRegistry, SlashCommand};

use crate::theme::Theme;

const MAX_VISIBLE: usize = 8;

pub struct Autocomplete {
    registry: CommandRegistry,
    matches: Vec<SlashCommand>,
    state: ListState,
    active: bool,
}

impl Default for Autocomplete {
    fn default() -> Self {
        Self::new()
    }
}

impl Autocomplete {
    pub fn new() -> Self {
        Self {
            registry: CommandRegistry::with_builtins(),
            matches: Vec::new(),
            state: ListState::default(),
            active: false,
        }
    }

    /// Recompute matches from the current input text.
    pub fn update(&mut self, input: &str) {
        match input.strip_prefix('/') {
            // Only while typing the command word (no space yet).
            Some(rest) if !rest.contains(char::is_whitespace) => {
                self.matches = self.registry.lookup(input).into_iter().copied().collect();
                self.active = !self.matches.is_empty();
                if self.active && self.state.selected().is_none() {
                    self.state.select(Some(0));
                }
                if !self.active {
                    self.state.select(None);
                }
            }
            _ => {
                self.active = false;
                self.matches.clear();
                self.state.select(None);
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn matches(&self) -> &[SlashCommand] {
        &self.matches
    }

    pub fn selected_index(&self) -> usize {
        self.state.selected().unwrap_or(0)
    }

    pub fn next(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let i = (self.selected_index() + 1) % self.matches.len();
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let i = self
            .selected_index()
            .checked_sub(1)
            .unwrap_or(self.matches.len() - 1);
        self.state.select(Some(i));
    }

    /// The selected command's usage template, to fill the input.
    pub fn confirm(&self) -> Option<&'static str> {
        self.matches.get(self.selected_index()).map(|c| c.usage)
    }

    pub fn dismiss(&mut self) {
        self.active = false;
    }

    /// Render anchored above `input_area`.
    pub fn render(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        if !self.active {
            return;
        }
        let n = self.matches.len().min(MAX_VISIBLE) as u16;
        let h = n + 2; // borders
        let area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(h),
            width: input_area.width.min(50),
            height: h,
        };
        let items: Vec<ListItem> = self
            .matches
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("/{:<10}", c.name),
                        Style::default().fg(theme.accent),
                    ),
                    Span::styled(c.description, Style::default().fg(theme.fg_subtle)),
                ]))
            })
            .collect();
        f.render_widget(Clear, area);
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent_secondary))
                    .style(Style::default().bg(theme.bg_secondary)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.accent)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, area, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_on_slash_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/th");
        assert!(ac.is_active());
        assert!(ac.matches().iter().any(|c| c.name == "theme"));
    }

    #[test]
    fn deactivates_without_slash_or_after_space() {
        let mut ac = Autocomplete::new();
        ac.update("hello");
        assert!(!ac.is_active());
        ac.update("/theme cyber"); // space typed -> past the command word
        assert!(!ac.is_active());
    }

    #[test]
    fn navigation_wraps_and_confirms() {
        let mut ac = Autocomplete::new();
        ac.update("/");
        assert!(!ac.matches().is_empty());
        ac.next();
        ac.prev();
        assert_eq!(ac.selected_index(), 0);
        assert!(ac.confirm().is_some());
    }
}
