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
const COMMAND_COLUMN_WIDTH: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletion {
    pub insert: String,
    pub placeholder: Option<&'static str>,
}

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
                if self.active {
                    // Clamp the selection into the new (possibly shorter)
                    // match list so confirm()/prev() never point past the end.
                    let sel = self
                        .state
                        .selected()
                        .unwrap_or(0)
                        .min(self.matches.len() - 1);
                    self.state.select(Some(sel));
                } else {
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

    pub fn selected_name(&self) -> Option<&'static str> {
        self.matches.get(self.selected_index()).map(|c| c.name)
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

    /// The selected command split into real input and optional ghost args.
    pub fn confirm(&self) -> Option<CommandCompletion> {
        self.matches
            .get(self.selected_index())
            .map(|c| completion_from_usage(c.usage))
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
        let area = popup_area(input_area, n);
        let items: Vec<ListItem> = self
            .matches
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("/{:<width$}", c.name, width = COMMAND_COLUMN_WIDTH),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(c.description, Style::default().fg(theme.fg_text)),
                ]))
            })
            .collect();
        f.render_widget(Clear, area);
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT)
                    .border_style(Style::default().fg(theme.separator))
                    .style(Style::default().bg(theme.bg_secondary)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.system)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, area, &mut self.state);
    }
}

fn completion_from_usage(usage: &'static str) -> CommandCompletion {
    match usage.split_once(char::is_whitespace) {
        Some((command, args)) => {
            let args = args.trim_start();
            CommandCompletion {
                insert: format!("{command} "),
                placeholder: (!args.is_empty()).then_some(args),
            }
        }
        None => CommandCompletion {
            insert: usage.to_string(),
            placeholder: None,
        },
    }
}

fn popup_area(input_area: Rect, visible_rows: u16) -> Rect {
    let h = visible_rows;
    Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(h),
        width: input_area.width,
        height: h,
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

    #[test]
    fn confirm_splits_usage_args_into_placeholder() {
        let mut ac = Autocomplete::new();
        ac.update("/sidebar");

        let completion = ac.confirm().expect("sidebar command completion");

        assert_eq!(completion.insert, "/sidebar ");
        assert_eq!(completion.placeholder, Some("[on|off|toggle|auto]"));
    }

    #[test]
    fn popup_uses_attached_full_width_palette_area() {
        let input_area = Rect::new(2, 20, 100, 4);
        let area = popup_area(input_area, 8);
        assert_eq!(area.x, 2);
        assert_eq!(area.width, 100);
        assert_eq!(area.height, 8);
        assert_eq!(area.y, 12);
    }

    #[test]
    fn render_uses_attached_command_palette_chrome() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = ratatui::backend::TestBackend::new(110, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input_area = Rect::new(0, 20, 110, 4);
        let mut ac = Autocomplete::new();

        ac.update("/");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("/theme"));
        assert!(content.contains("Switch the TUI theme"));
        assert!(!content.contains("Commands ·"));
    }

    #[test]
    fn exposes_selected_command_name() {
        let mut ac = Autocomplete::new();
        ac.update("/theme");
        assert_eq!(ac.selected_name(), Some("theme"));
    }

    #[test]
    fn rerender_clears_stale_rows_when_matches_shrink() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input_area = Rect::new(0, 20, 100, 4);
        let mut ac = Autocomplete::new();

        ac.update("/");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();
        ac.update("/theme");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("/theme"));
        assert!(!content.contains("/compact"));
        assert!(!content.contains("/cost"));
    }
}
