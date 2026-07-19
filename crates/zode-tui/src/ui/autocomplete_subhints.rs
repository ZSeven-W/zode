//! Secondary "subcommand hint" popup machinery shared by the `/op <sub>`
//! and `/browser <sub>` autocomplete modes in [`super::autocomplete`].
//!
//! Both modes activate on their own `/<name> ` prefix and filter a static
//! `(name, description)` table as the user types a subcommand. The only
//! per-mode differences are the table itself, the inserted prefix (`"/op "`
//! vs `"/browser "`), and which subcommand names take a required argument
//! (and so get a trailing space on confirm). [`SubHints`] captures the
//! shared filter/navigate/confirm/render behavior once; [`Autocomplete`]
//! (in `autocomplete.rs`) owns one instance per mode.
//!
//! [`Autocomplete`]: super::autocomplete::Autocomplete

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::theme::Theme;

use super::autocomplete::{popup_area, COMMAND_COLUMN_WIDTH, MAX_VISIBLE, NAME_COLUMN_MAX};

/// `/op` hint entries. `/op <free text>` is the primary design-generation
/// command, so the popup only advertises diagnostics plus the raw-call escape
/// hatch. Legacy `design` / `generate` compatibility paths remain parser-only.
pub const OP_SUBCOMMANDS: &[&str] = &["status", "call"];

/// Brief descriptions shown alongside each `/op` entry in the hint popup.
pub(crate) const OP_SUBCOMMAND_DESCS: &[&str] =
    &["report connection state", "call an MCP tool by name"];

/// `/op` entries that take a required argument, so `SubHints::confirm` should
/// leave a trailing space instead of submitting bare.
pub(crate) const OP_SUB_TRAILING_SPACE: &[&str] = &["call"];

/// Known `/browser` subcommands — mirrors `zode_core::commands::browser::
/// map_subcommand`, kept here so the hint popup never goes stale relative to
/// the parser.
pub(crate) const BROWSER_SUBCOMMANDS: &[&str] =
    &["status", "launch", "close", "pair", "target", "screenshot"];

/// Brief descriptions shown alongside each `/browser` subcommand in the hint popup.
pub(crate) const BROWSER_SUBCOMMAND_DESCS: &[&str] = &[
    "Connection and target state",
    "Launch the managed browser now",
    "Close the managed browser",
    "Pair the Chrome extension (M2)",
    "Switch target: managed | bridge",
    "Capture a screenshot to a file",
];

/// `/browser` subcommands that take a required argument, so `SubHints::confirm`
/// should leave a trailing space instead of submitting bare.
pub(crate) const BROWSER_SUB_TRAILING_SPACE: &[&str] = &["target", "screenshot"];

/// Known `/loop` subcommands — mirrors `zode_core::commands::loop_sched::
/// parse_loop`'s `list`/`stop` branches (the interval-start form has no fixed
/// subcommand word, so it isn't listed here).
pub(crate) const LOOP_SUBCOMMANDS: &[&str] = &["list", "stop"];

/// Brief descriptions shown alongside each `/loop` subcommand in the hint popup.
pub(crate) const LOOP_SUBCOMMAND_DESCS: &[&str] = &[
    "List running loop jobs",
    "Stop a loop job (all, if no id given)",
];

/// `/loop` entries that take a required argument, so `SubHints::confirm`
/// should leave a trailing space instead of submitting bare. `stop`'s id is
/// optional (bare `/loop stop` stops every job), so it's excluded here.
pub(crate) const LOOP_SUB_TRAILING_SPACE: &[&str] = &[];

/// Known `/schedule` subcommands — mirrors `zode_core::commands::loop_sched::
/// parse_schedule`'s dispatch arms.
pub(crate) const SCHEDULE_SUBCOMMANDS: &[&str] = &["add", "list", "rm", "enable", "disable"];

/// Brief descriptions shown alongside each `/schedule` subcommand in the hint popup.
pub(crate) const SCHEDULE_SUBCOMMAND_DESCS: &[&str] = &[
    "Add a schedule (hh:mm | mon hh:mm | every 2h) <prompt>",
    "List persisted schedule jobs",
    "Remove a schedule by id",
    "Re-enable a disabled schedule by id",
    "Disable a schedule by id",
];

/// `/schedule` entries that take a required argument, so `SubHints::confirm`
/// should leave a trailing space instead of submitting bare.
pub(crate) const SCHEDULE_SUB_TRAILING_SPACE: &[&str] = &["add", "rm", "enable", "disable"];

/// One secondary subcommand-hint popup, e.g. everything shown while typing
/// `/op <sub>` or `/browser <sub>`. Parameterized at construction by the
/// inserted `prefix` (`"/op "` / `"/browser "`) and the subset of names that
/// need a trailing space on confirm.
pub(crate) struct SubHints {
    prefix: &'static str,
    trailing_space_for: &'static [&'static str],
    pub(crate) matches: Vec<(&'static str, &'static str)>,
    state: ListState,
    pub(crate) active: bool,
}

impl SubHints {
    pub(crate) fn new(prefix: &'static str, trailing_space_for: &'static [&'static str]) -> Self {
        Self {
            prefix,
            trailing_space_for,
            matches: Vec::new(),
            state: ListState::default(),
            active: false,
        }
    }

    /// Recompute `matches` from `table` filtered by the typed subcommand
    /// prefix (case-insensitive substring match), clamping the selection
    /// into the new (possibly shorter) list.
    pub(crate) fn update(&mut self, table: &[(&'static str, &'static str)], typed: &str) {
        let q = typed.to_ascii_lowercase();
        self.matches = table
            .iter()
            .filter(|(name, _)| q.is_empty() || name.contains(q.as_str()))
            .copied()
            .collect();
        let total = self.matches.len();
        self.active = total > 0;
        if self.active {
            let sel = self.state.selected().unwrap_or(0).min(total - 1);
            self.state.select(Some(sel));
        } else {
            self.state.select(None);
        }
    }

    pub(crate) fn next(&mut self) {
        let total = self.matches.len();
        if total == 0 {
            return;
        }
        let i = (self.state.selected().unwrap_or(0) + 1) % total;
        self.state.select(Some(i));
    }

    pub(crate) fn prev(&mut self) {
        let total = self.matches.len();
        if total == 0 {
            return;
        }
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .checked_sub(1)
            .unwrap_or(total - 1);
        self.state.select(Some(i));
    }

    /// Confirm the selected subcommand; returns the text to insert (e.g.
    /// `"/op design "` or `"/browser status"`).
    pub(crate) fn confirm(&self) -> Option<String> {
        let i = self.state.selected().unwrap_or(0);
        self.matches.get(i).map(|(name, _)| {
            if self.trailing_space_for.contains(name) {
                format!("{}{name} ", self.prefix)
            } else {
                format!("{}{name}", self.prefix)
            }
        })
    }

    pub(crate) fn dismiss(&mut self) {
        self.active = false;
        self.matches.clear();
        self.state.select(None);
    }

    /// Render this popup above `input_area`.
    pub(crate) fn render(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        let n = self.matches.len().min(MAX_VISIBLE) as u16;
        if n == 0 {
            return;
        }
        let area = popup_area(input_area, n);
        // Name column width: longest subcommand name, clamped for readability.
        let name_col = self
            .matches
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0)
            .clamp(COMMAND_COLUMN_WIDTH, NAME_COLUMN_MAX);
        let label = format!("  {}", self.prefix);
        let items: Vec<ListItem> = self
            .matches
            .iter()
            .map(|(name, desc)| {
                ListItem::new(Line::from(vec![
                    Span::raw(label.clone()),
                    Span::styled(
                        format!("{name:<name_col$} "),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(crate::tr(desc), Style::default().fg(theme.fg_text)),
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
                    .bg(theme.accent)
                    .fg(theme.bg_secondary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, area, &mut self.state);
    }
}
