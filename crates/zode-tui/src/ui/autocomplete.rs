//! Slash-command autocomplete popup. Activates when the input line starts
//! with "/", filters via CommandRegistry::lookup, navigable with ↑↓/Tab,
//! confirmed with Enter/Tab, dismissed with Esc.
//!
//! Also provides a secondary subcommand-hint popup for `/op <subcommand>`:
//! activates when the input starts with "/op " and filters against
//! `OP_SUBCOMMANDS`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use zode_core::commands::{CommandRegistry, SlashCommand};

use crate::theme::Theme;

const MAX_VISIBLE: usize = 8;
const COMMAND_COLUMN_WIDTH: usize = 15;
/// Upper bound for the auto-sized name column, so one very long name can't push
/// every description off-screen.
const NAME_COLUMN_MAX: usize = 30;

/// Known `/op` subcommands. Each maps to an MCP tool on the OpenPencil side
/// (or to the built-in `status` report). Kept here so the hint popup never
/// goes stale relative to the parser in `zode_core::commands::op`.
pub const OP_SUBCOMMANDS: &[&str] = &[
    "status",
    "design",
    "generate",
    "insert",
    "update",
    "delete",
    "move",
    "copy",
    "page",
    "vars",
    "selection",
    "call",
];

/// Brief descriptions shown alongside each `/op` subcommand in the hint popup.
const OP_SUBCOMMAND_DESCS: &[&str] = &[
    "report connection state",
    "run batch_design DSL",
    "generate a page from a prompt",
    "insert a node",
    "update node properties",
    "delete nodes",
    "move nodes",
    "copy nodes",
    "page operations",
    "variable operations",
    "selection operations",
    "call any MCP tool by name",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletion {
    pub insert: String,
    pub placeholder: Option<&'static str>,
}

/// A dynamic slash command surfaced alongside the built-ins: a user-defined
/// sub-agent, a skill, or an MCP tool. Selecting one submits a templated turn
/// (see app.rs `expand_dynamic_command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynCmd {
    pub name: String,
    /// Short tag shown after the description: "agent" | "skill" | "MCP".
    pub kind: &'static str,
    pub description: String,
}

pub struct Autocomplete {
    registry: CommandRegistry,
    matches: Vec<SlashCommand>,
    /// All known dynamic commands (refreshed from the engine on assembly).
    dynamic: Vec<DynCmd>,
    /// Dynamic commands matching the current prefix (shown after builtins).
    dyn_matches: Vec<DynCmd>,
    state: ListState,
    active: bool,
    /// Secondary mode: subcommand hints for `/op <sub>`.
    op_sub_matches: Vec<(&'static str, &'static str)>,
    op_sub_state: ListState,
    op_sub_active: bool,
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
            dynamic: Vec::new(),
            dyn_matches: Vec::new(),
            state: ListState::default(),
            active: false,
            op_sub_matches: Vec::new(),
            op_sub_state: ListState::default(),
            op_sub_active: false,
        }
    }

    /// Replace the dynamic command set (user agents, skills, MCP tools). Called
    /// after the engine assembles/reassembles, since those change.
    pub fn set_dynamic(&mut self, dynamic: Vec<DynCmd>) {
        self.dynamic = dynamic;
    }

    /// Total navigable rows: builtins then dynamics.
    fn total(&self) -> usize {
        self.matches.len() + self.dyn_matches.len()
    }

    /// Recompute matches from the current input text.
    ///
    /// Two modes:
    /// 1. Primary: `/command` (no space) — shows matching slash commands.
    /// 2. Secondary: `/op ` prefix — shows `/op` subcommand hints.
    pub fn update(&mut self, input: &str) {
        // Secondary mode: `/op ` prefix triggers subcommand hints.
        if let Some(sub_prefix) = input.strip_prefix("/op ") {
            self.active = false;
            self.matches.clear();
            self.dyn_matches.clear();
            self.state.select(None);
            self.update_op_sub(sub_prefix);
            return;
        }
        // Clear any stale secondary state when not in `/op ` mode.
        self.op_sub_active = false;
        self.op_sub_matches.clear();
        self.op_sub_state.select(None);

        match input.strip_prefix('/') {
            // Only while typing the command word (no space yet).
            Some(rest) if !rest.contains(char::is_whitespace) => {
                self.matches = self.registry.lookup(input).into_iter().copied().collect();
                let q = rest.to_ascii_lowercase();
                self.dyn_matches = self
                    .dynamic
                    .iter()
                    .filter(|d| {
                        q.is_empty()
                            || d.name.to_ascii_lowercase().contains(&q)
                            || d.description.to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect();
                let total = self.total();
                self.active = total > 0;
                if self.active {
                    // Clamp the selection into the new (possibly shorter) list
                    // so confirm()/prev() never point past the end.
                    let sel = self.state.selected().unwrap_or(0).min(total - 1);
                    self.state.select(Some(sel));
                } else {
                    self.state.select(None);
                }
            }
            _ => {
                self.active = false;
                self.matches.clear();
                self.dyn_matches.clear();
                self.state.select(None);
            }
        }
    }

    /// Recompute the `/op` subcommand hint list from the typed subcommand prefix.
    fn update_op_sub(&mut self, typed: &str) {
        let q = typed.to_ascii_lowercase();
        self.op_sub_matches = OP_SUBCOMMANDS
            .iter()
            .zip(OP_SUBCOMMAND_DESCS.iter())
            .filter(|(name, _)| q.is_empty() || name.contains(q.as_str()))
            .map(|(n, d)| (*n, *d))
            .collect();
        let total = self.op_sub_matches.len();
        self.op_sub_active = total > 0;
        if self.op_sub_active {
            let sel = self.op_sub_state.selected().unwrap_or(0).min(total - 1);
            self.op_sub_state.select(Some(sel));
        } else {
            self.op_sub_state.select(None);
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// True when the `/op` subcommand hint popup is showing.
    pub fn is_op_sub_active(&self) -> bool {
        self.op_sub_active
    }

    pub fn matches(&self) -> &[SlashCommand] {
        &self.matches
    }

    pub fn selected_index(&self) -> usize {
        self.state.selected().unwrap_or(0)
    }

    /// The selected command's name (builtin or dynamic), if any.
    pub fn selected_name(&self) -> Option<&str> {
        let i = self.selected_index();
        if i < self.matches.len() {
            Some(self.matches[i].name)
        } else {
            self.dyn_matches
                .get(i - self.matches.len())
                .map(|d| d.name.as_str())
        }
    }

    pub fn next(&mut self) {
        let total = self.total();
        if total == 0 {
            return;
        }
        let i = (self.selected_index() + 1) % total;
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        let total = self.total();
        if total == 0 {
            return;
        }
        let i = self.selected_index().checked_sub(1).unwrap_or(total - 1);
        self.state.select(Some(i));
    }

    /// The selected command split into real input and optional ghost args.
    /// Dynamic commands insert `/name ` (no placeholder).
    pub fn confirm(&self) -> Option<CommandCompletion> {
        let i = self.selected_index();
        if i < self.matches.len() {
            self.matches.get(i).map(|c| completion_from_usage(c.usage))
        } else {
            self.dyn_matches
                .get(i - self.matches.len())
                .map(|d| CommandCompletion {
                    insert: format!("/{} ", d.name),
                    placeholder: None,
                })
        }
    }

    /// Navigate down in the `/op` subcommand hint list.
    pub fn op_sub_next(&mut self) {
        let total = self.op_sub_matches.len();
        if total == 0 {
            return;
        }
        let i = (self.op_sub_state.selected().unwrap_or(0) + 1) % total;
        self.op_sub_state.select(Some(i));
    }

    /// Navigate up in the `/op` subcommand hint list.
    pub fn op_sub_prev(&mut self) {
        let total = self.op_sub_matches.len();
        if total == 0 {
            return;
        }
        let i = self
            .op_sub_state
            .selected()
            .unwrap_or(0)
            .checked_sub(1)
            .unwrap_or(total - 1);
        self.op_sub_state.select(Some(i));
    }

    /// Confirm the selected `/op` subcommand; returns the text to insert
    /// (e.g. `"/op status"` or `"/op design "`).
    pub fn op_sub_confirm(&self) -> Option<String> {
        let i = self.op_sub_state.selected().unwrap_or(0);
        self.op_sub_matches.get(i).map(|(name, _)| {
            // `design`, `generate`, and `call` take a required argument, so
            // leave a trailing space; all others can be submitted as-is.
            if *name == "design" || *name == "generate" || *name == "call" {
                format!("/op {name} ")
            } else {
                format!("/op {name}")
            }
        })
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.op_sub_active = false;
        self.op_sub_matches.clear();
        self.op_sub_state.select(None);
    }

    /// Render anchored above `input_area`.
    /// Renders the primary command popup when active, or the `/op` subcommand
    /// hint popup when `op_sub_active`, but never both simultaneously.
    pub fn render(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        if self.op_sub_active {
            self.render_op_sub(f, input_area, theme);
            return;
        }
        if !self.active {
            return;
        }
        let n = self.total().min(MAX_VISIBLE) as u16;
        let area = popup_area(input_area, n);
        // Shared name column so descriptions align with a gap, sized to the
        // longest visible command name (clamped) — long skill names no longer
        // collide with their description.
        let name_col = self
            .matches
            .iter()
            .map(|c| c.name.chars().count())
            .chain(self.dyn_matches.iter().map(|d| d.name.chars().count()))
            .max()
            .unwrap_or(0)
            .clamp(COMMAND_COLUMN_WIDTH, NAME_COLUMN_MAX);
        let mut items: Vec<ListItem> = self
            .matches
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("/{:<name_col$} ", c.name),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        zode_core::i18n::t(c.description),
                        Style::default().fg(theme.fg_text),
                    ),
                ]))
            })
            .collect();
        // Dynamic commands (agents / skills / MCP) after the builtins, each
        // tagged with its kind.
        for d in &self.dyn_matches {
            items.push(ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("/{:<name_col$} ", d.name),
                    Style::default()
                        .fg(theme.accent_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} ", d.description),
                    Style::default().fg(theme.fg_text),
                ),
                Span::styled(
                    format!("({})", d.kind),
                    Style::default().fg(theme.fg_subtle),
                ),
            ])));
        }
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

    /// Render the `/op` subcommand hint popup above `input_area`.
    fn render_op_sub(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        let n = self.op_sub_matches.len().min(MAX_VISIBLE) as u16;
        if n == 0 {
            return;
        }
        let area = popup_area(input_area, n);
        // Name column width: longest subcommand name, clamped for readability.
        let name_col = self
            .op_sub_matches
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0)
            .clamp(COMMAND_COLUMN_WIDTH, NAME_COLUMN_MAX);
        let items: Vec<ListItem> = self
            .op_sub_matches
            .iter()
            .map(|(name, desc)| {
                ListItem::new(Line::from(vec![
                    Span::raw("  /op "),
                    Span::styled(
                        format!("{name:<name_col$} "),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(*desc, Style::default().fg(theme.fg_text)),
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
        f.render_stateful_widget(list, area, &mut self.op_sub_state);
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
    fn dynamic_commands_match_and_confirm() {
        let mut ac = Autocomplete::new();
        ac.set_dynamic(vec![DynCmd {
            name: "refactorer".into(),
            kind: "agent",
            description: "Refactors".into(),
        }]);
        ac.update("/refac");
        assert!(ac.is_active());
        // The dynamic match is selectable past the (zero) builtin matches.
        assert_eq!(ac.selected_name(), Some("refactorer"));
        let c = ac.confirm().expect("dynamic completion");
        assert_eq!(c.insert, "/refactorer ");
        assert_eq!(c.placeholder, None);
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
        // Assert on the first command, which is always in the rendered viewport
        // regardless of how many commands the registry grows to (the palette is
        // height-clipped, so a mid-list item like /theme can scroll out).
        assert!(content.contains("/help"));
        assert!(content.contains("Show commands and keybindings"));
        assert!(!content.contains("Commands ·"));
    }

    #[test]
    fn selected_row_uses_theme_accent_background() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        assert_ne!(theme.accent, theme.system);
        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input_area = Rect::new(0, 20, 100, 4);
        let mut ac = Autocomplete::new();

        ac.update("/plugin");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

        let buf = term.backend().buffer();
        let (row, col) = (0..buf.area.height)
            .find_map(|y| {
                let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
                row.find("/plugin").map(|x| (y, x as u16))
            })
            .expect("selected plugin row should render");
        assert_eq!(buf[(col, row)].bg, theme.accent);
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

    // --- /op subcommand hint tests ---

    #[test]
    fn op_sub_activates_on_op_space_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        assert!(ac.is_op_sub_active());
        assert!(!ac.is_active(), "primary popup should be off in op-sub mode");
    }

    #[test]
    fn op_sub_filters_by_typed_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/op des");
        assert!(ac.is_op_sub_active());
        let confirmed = ac.op_sub_confirm().expect("should match 'design'");
        assert!(confirmed.contains("design"));
    }

    #[test]
    fn op_sub_confirm_appends_space_for_design_and_call() {
        let mut ac = Autocomplete::new();
        // Navigate to "design" (it is second after "status"; easier to filter directly).
        ac.update("/op design");
        assert!(ac.is_op_sub_active());
        let text = ac.op_sub_confirm().expect("design match");
        assert_eq!(text, "/op design ");

        ac.update("/op call");
        let text = ac.op_sub_confirm().expect("call match");
        assert_eq!(text, "/op call ");
    }

    #[test]
    fn op_sub_confirm_no_trailing_space_for_status() {
        let mut ac = Autocomplete::new();
        ac.update("/op status");
        assert!(ac.is_op_sub_active());
        let text = ac.op_sub_confirm().expect("status match");
        assert_eq!(text, "/op status");
    }

    #[test]
    fn op_sub_navigation_wraps() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        let total = ac.op_sub_matches.len();
        assert!(total > 1);
        ac.op_sub_next();
        ac.op_sub_prev();
        // Back to first entry after next+prev.
        let text = ac.op_sub_confirm().expect("wrapped back to first");
        assert!(text.contains("status"));
    }

    #[test]
    fn op_sub_dismiss_clears_state() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        assert!(ac.is_op_sub_active());
        ac.dismiss();
        assert!(!ac.is_op_sub_active());
    }

    #[test]
    fn op_sub_deactivates_when_leaving_op_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        assert!(ac.is_op_sub_active());
        ac.update("/th");
        assert!(!ac.is_op_sub_active());
        assert!(ac.is_active()); // switched back to primary popup
    }

    #[test]
    fn op_sub_renders_subcommand_names() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = ratatui::backend::TestBackend::new(110, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input_area = Rect::new(0, 20, 110, 4);
        let mut ac = Autocomplete::new();

        ac.update("/op ");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("status"), "popup should show 'status'");
        assert!(content.contains("design"), "popup should show 'design'");
    }

    #[test]
    fn op_subcommands_constant_covers_all_expected_entries() {
        let expected = [
            "status", "design", "generate", "insert", "update", "delete", "move", "copy", "page",
            "vars", "selection", "call",
        ];
        assert_eq!(OP_SUBCOMMANDS, expected);
        assert_eq!(
            OP_SUBCOMMANDS.len(),
            OP_SUBCOMMAND_DESCS.len(),
            "each subcommand must have a description"
        );
    }
}
