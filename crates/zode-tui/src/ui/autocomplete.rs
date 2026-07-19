//! Slash-command autocomplete popup. Activates when the input line starts
//! with "/", filters via CommandRegistry::lookup, navigable with ↑↓/Tab,
//! confirmed with Enter/Tab, dismissed with Esc.
//!
//! Also provides secondary subcommand-hint popups for `/op <subcommand>` and
//! `/browser <subcommand>`: each activates when the input starts with its
//! `/<name> ` prefix and filters against the matching `*_SUBCOMMANDS` table.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use zode_core::commands::{CommandRegistry, SlashCommand};

use crate::theme::Theme;

use super::autocomplete_subhints::{
    SubHints, BROWSER_SUBCOMMANDS, BROWSER_SUBCOMMAND_DESCS, BROWSER_SUB_TRAILING_SPACE,
    LOOP_SUBCOMMANDS, LOOP_SUBCOMMAND_DESCS, LOOP_SUB_TRAILING_SPACE, OP_SUBCOMMANDS,
    OP_SUBCOMMAND_DESCS, OP_SUB_TRAILING_SPACE, SCHEDULE_SUBCOMMANDS, SCHEDULE_SUBCOMMAND_DESCS,
    SCHEDULE_SUB_TRAILING_SPACE,
};

pub(crate) const MAX_VISIBLE: usize = 8;
pub(crate) const COMMAND_COLUMN_WIDTH: usize = 15;
/// Upper bound for the auto-sized name column, so one very long name can't push
/// every description off-screen.
pub(crate) const NAME_COLUMN_MAX: usize = 30;

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
    op_sub: SubHints,
    /// Secondary mode: subcommand hints for `/browser <sub>`.
    browser_sub: SubHints,
    /// Secondary mode: subcommand hints for `/loop <sub>`.
    loop_sub: SubHints,
    /// Secondary mode: subcommand hints for `/schedule <sub>`.
    schedule_sub: SubHints,
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
            op_sub: SubHints::new("/op ", OP_SUB_TRAILING_SPACE),
            browser_sub: SubHints::new("/browser ", BROWSER_SUB_TRAILING_SPACE),
            loop_sub: SubHints::new("/loop ", LOOP_SUB_TRAILING_SPACE),
            schedule_sub: SubHints::new("/schedule ", SCHEDULE_SUB_TRAILING_SPACE),
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
    /// Five modes:
    /// 1. Primary: `/command` (no space) — shows matching slash commands.
    /// 2. Secondary: `/op ` prefix — shows `/op` subcommand hints.
    /// 3. Secondary: `/browser ` prefix — shows `/browser` subcommand hints.
    /// 4. Secondary: `/loop ` prefix — shows `/loop` subcommand hints.
    /// 5. Secondary: `/schedule ` prefix — shows `/schedule` subcommand hints.
    pub fn update(&mut self, input: &str) {
        // Secondary mode: `/op ` prefix triggers subcommand hints.
        if let Some(sub_prefix) = input.strip_prefix("/op ") {
            self.active = false;
            self.matches.clear();
            self.dyn_matches.clear();
            self.state.select(None);
            self.browser_sub.dismiss();
            self.loop_sub.dismiss();
            self.schedule_sub.dismiss();
            let table: Vec<_> = OP_SUBCOMMANDS
                .iter()
                .copied()
                .zip(OP_SUBCOMMAND_DESCS.iter().copied())
                .collect();
            self.op_sub.update(&table, sub_prefix);
            return;
        }
        // Secondary mode: `/browser ` prefix triggers subcommand hints.
        if let Some(sub_prefix) = input.strip_prefix("/browser ") {
            self.active = false;
            self.matches.clear();
            self.dyn_matches.clear();
            self.state.select(None);
            self.op_sub.dismiss();
            self.loop_sub.dismiss();
            self.schedule_sub.dismiss();
            let table: Vec<_> = BROWSER_SUBCOMMANDS
                .iter()
                .copied()
                .zip(BROWSER_SUBCOMMAND_DESCS.iter().copied())
                .collect();
            self.browser_sub.update(&table, sub_prefix);
            return;
        }
        // Secondary mode: `/loop ` prefix triggers subcommand hints.
        if let Some(sub_prefix) = input.strip_prefix("/loop ") {
            self.active = false;
            self.matches.clear();
            self.dyn_matches.clear();
            self.state.select(None);
            self.op_sub.dismiss();
            self.browser_sub.dismiss();
            self.schedule_sub.dismiss();
            let table: Vec<_> = LOOP_SUBCOMMANDS
                .iter()
                .copied()
                .zip(LOOP_SUBCOMMAND_DESCS.iter().copied())
                .collect();
            self.loop_sub.update(&table, sub_prefix);
            return;
        }
        // Secondary mode: `/schedule ` prefix triggers subcommand hints.
        if let Some(sub_prefix) = input.strip_prefix("/schedule ") {
            self.active = false;
            self.matches.clear();
            self.dyn_matches.clear();
            self.state.select(None);
            self.op_sub.dismiss();
            self.browser_sub.dismiss();
            self.loop_sub.dismiss();
            let table: Vec<_> = SCHEDULE_SUBCOMMANDS
                .iter()
                .copied()
                .zip(SCHEDULE_SUBCOMMAND_DESCS.iter().copied())
                .collect();
            self.schedule_sub.update(&table, sub_prefix);
            return;
        }
        // Clear any stale secondary state when not in a subcommand-hint mode.
        self.op_sub.dismiss();
        self.browser_sub.dismiss();
        self.loop_sub.dismiss();
        self.schedule_sub.dismiss();

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

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// True when the `/op` subcommand hint popup is showing.
    pub fn is_op_sub_active(&self) -> bool {
        self.op_sub.active
    }

    /// True when the `/browser` subcommand hint popup is showing.
    pub fn is_browser_sub_active(&self) -> bool {
        self.browser_sub.active
    }

    /// True when the `/loop` subcommand hint popup is showing.
    pub fn is_loop_sub_active(&self) -> bool {
        self.loop_sub.active
    }

    /// True when the `/schedule` subcommand hint popup is showing.
    pub fn is_schedule_sub_active(&self) -> bool {
        self.schedule_sub.active
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
        self.op_sub.next();
    }

    /// Navigate up in the `/op` subcommand hint list.
    pub fn op_sub_prev(&mut self) {
        self.op_sub.prev();
    }

    /// Confirm the selected `/op` subcommand; returns the text to insert
    /// (e.g. `"/op status"` or `"/op design "`).
    pub fn op_sub_confirm(&self) -> Option<String> {
        self.op_sub.confirm()
    }

    /// Navigate down in the `/browser` subcommand hint list.
    pub fn browser_sub_next(&mut self) {
        self.browser_sub.next();
    }

    /// Navigate up in the `/browser` subcommand hint list.
    pub fn browser_sub_prev(&mut self) {
        self.browser_sub.prev();
    }

    /// Confirm the selected `/browser` subcommand; returns the text to insert
    /// (e.g. `"/browser status"` or `"/browser target "`).
    pub fn browser_sub_confirm(&self) -> Option<String> {
        self.browser_sub.confirm()
    }

    /// Navigate down in the `/loop` subcommand hint list.
    pub fn loop_sub_next(&mut self) {
        self.loop_sub.next();
    }

    /// Navigate up in the `/loop` subcommand hint list.
    pub fn loop_sub_prev(&mut self) {
        self.loop_sub.prev();
    }

    /// Confirm the selected `/loop` subcommand; returns the text to insert
    /// (e.g. `"/loop list"` or `"/loop stop"`).
    pub fn loop_sub_confirm(&self) -> Option<String> {
        self.loop_sub.confirm()
    }

    /// Navigate down in the `/schedule` subcommand hint list.
    pub fn schedule_sub_next(&mut self) {
        self.schedule_sub.next();
    }

    /// Navigate up in the `/schedule` subcommand hint list.
    pub fn schedule_sub_prev(&mut self) {
        self.schedule_sub.prev();
    }

    /// Confirm the selected `/schedule` subcommand; returns the text to insert
    /// (e.g. `"/schedule list"` or `"/schedule add "`).
    pub fn schedule_sub_confirm(&self) -> Option<String> {
        self.schedule_sub.confirm()
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.op_sub.dismiss();
        self.browser_sub.dismiss();
        self.loop_sub.dismiss();
        self.schedule_sub.dismiss();
    }

    /// Render anchored above `input_area`.
    /// Renders the primary command popup when active, or one of the
    /// secondary subcommand-hint popups (`/op`, `/browser`, `/loop`,
    /// `/schedule`), but never more than one at a time.
    pub fn render(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        if self.op_sub.active {
            self.op_sub.render(f, input_area, theme);
            return;
        }
        if self.browser_sub.active {
            self.browser_sub.render(f, input_area, theme);
            return;
        }
        if self.loop_sub.active {
            self.loop_sub.render(f, input_area, theme);
            return;
        }
        if self.schedule_sub.active {
            self.schedule_sub.render(f, input_area, theme);
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

pub(crate) fn popup_area(input_area: Rect, visible_rows: u16) -> Rect {
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
        assert_eq!(
            completion.placeholder,
            Some("[on|off|toggle|auto|mcp|files|todo]")
        );
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
        assert!(
            !ac.is_active(),
            "primary popup should be off in op-sub mode"
        );
    }

    #[test]
    fn op_sub_filters_by_typed_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/op sta");
        assert!(ac.is_op_sub_active());
        let confirmed = ac.op_sub_confirm().expect("should match 'status'");
        assert_eq!(confirmed, "/op status");
    }

    #[test]
    fn op_sub_confirm_appends_space_for_call_only() {
        let mut ac = Autocomplete::new();
        ac.update("/op call");
        assert!(ac.is_op_sub_active());
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
        let total = ac.op_sub.matches.len();
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
    fn op_sub_renders_user_facing_entries() {
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
        assert!(
            content.contains("call"),
            "popup should show hidden raw-call escape hatch"
        );
        assert!(
            !content.contains("design"),
            "popup should not advertise legacy direct DSL"
        );
        assert!(
            !content.contains("generate"),
            "popup should not advertise prompt alias"
        );
        assert!(
            !content.contains("get_document_info"),
            "popup should hide raw MCP tools"
        );
    }

    #[test]
    fn op_subcommands_constant_covers_public_entries() {
        let expected = ["status", "call"];
        assert_eq!(OP_SUBCOMMANDS, expected);
        assert_eq!(OP_SUBCOMMANDS.len(), OP_SUBCOMMAND_DESCS.len(),);
    }

    // --- /browser subcommand hint tests ---

    #[test]
    fn browser_subcommand_tables_align() {
        assert_eq!(BROWSER_SUBCOMMANDS.len(), BROWSER_SUBCOMMAND_DESCS.len());
    }

    #[test]
    fn browser_sub_filter_and_confirm() {
        let mut ac = Autocomplete::default();
        ac.update("/browser st");
        assert!(ac.is_browser_sub_active());
        assert_eq!(ac.browser_sub_confirm().unwrap(), "/browser status");
    }

    #[test]
    fn browser_sub_activates_on_browser_space_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/browser ");
        assert!(ac.is_browser_sub_active());
        assert!(
            !ac.is_active(),
            "primary popup should be off in browser-sub mode"
        );
    }

    #[test]
    fn browser_sub_confirm_appends_space_for_target_and_screenshot() {
        let mut ac = Autocomplete::new();
        ac.update("/browser target");
        assert!(ac.is_browser_sub_active());
        let text = ac.browser_sub_confirm().expect("target match");
        assert_eq!(text, "/browser target ");

        ac.update("/browser screenshot");
        let text = ac.browser_sub_confirm().expect("screenshot match");
        assert_eq!(text, "/browser screenshot ");
    }

    #[test]
    fn browser_sub_confirm_no_trailing_space_for_status() {
        let mut ac = Autocomplete::new();
        ac.update("/browser status");
        assert!(ac.is_browser_sub_active());
        let text = ac.browser_sub_confirm().expect("status match");
        assert_eq!(text, "/browser status");
    }

    #[test]
    fn browser_sub_navigation_wraps() {
        let mut ac = Autocomplete::new();
        ac.update("/browser ");
        let total = ac.browser_sub.matches.len();
        assert!(total > 1);
        ac.browser_sub_next();
        ac.browser_sub_prev();
        let text = ac.browser_sub_confirm().expect("wrapped back to first");
        assert!(text.contains("status"));
    }

    #[test]
    fn browser_sub_dismiss_clears_state() {
        let mut ac = Autocomplete::new();
        ac.update("/browser ");
        assert!(ac.is_browser_sub_active());
        ac.dismiss();
        assert!(!ac.is_browser_sub_active());
    }

    #[test]
    fn browser_sub_deactivates_when_leaving_browser_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/browser ");
        assert!(ac.is_browser_sub_active());
        ac.update("/th");
        assert!(!ac.is_browser_sub_active());
        assert!(ac.is_active()); // switched back to primary popup
    }

    #[test]
    fn op_sub_and_browser_sub_are_mutually_exclusive() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        assert!(ac.is_op_sub_active());
        ac.update("/browser ");
        assert!(ac.is_browser_sub_active());
        assert!(!ac.is_op_sub_active());
    }

    #[test]
    fn browser_sub_renders_subcommand_names() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = ratatui::backend::TestBackend::new(110, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input_area = Rect::new(0, 20, 110, 4);
        let mut ac = Autocomplete::new();

        ac.update("/browser ");
        term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("status"), "popup should show 'status'");
        assert!(content.contains("launch"), "popup should show 'launch'");
    }

    // --- /loop subcommand hint tests ---

    #[test]
    fn loop_subcommand_tables_align() {
        assert_eq!(LOOP_SUBCOMMANDS.len(), LOOP_SUBCOMMAND_DESCS.len());
    }

    #[test]
    fn loop_sub_activates_on_loop_space_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/loop ");
        assert!(ac.is_loop_sub_active());
        assert!(
            !ac.is_active(),
            "primary popup should be off in loop-sub mode"
        );
    }

    #[test]
    fn loop_sub_filter_and_confirm() {
        let mut ac = Autocomplete::default();
        ac.update("/loop li");
        assert!(ac.is_loop_sub_active());
        assert_eq!(ac.loop_sub_confirm().unwrap(), "/loop list");
    }

    #[test]
    fn loop_sub_confirm_no_trailing_space_for_stop() {
        let mut ac = Autocomplete::new();
        ac.update("/loop stop");
        let text = ac.loop_sub_confirm().expect("stop match");
        // `stop`'s id is optional (bare `/loop stop` stops every job), so no
        // trailing space is forced the way `/browser target` gets one.
        assert_eq!(text, "/loop stop");
    }

    // --- /schedule subcommand hint tests ---

    #[test]
    fn schedule_subcommand_tables_align() {
        assert_eq!(SCHEDULE_SUBCOMMANDS.len(), SCHEDULE_SUBCOMMAND_DESCS.len());
    }

    #[test]
    fn schedule_sub_activates_on_schedule_space_prefix() {
        let mut ac = Autocomplete::new();
        ac.update("/schedule ");
        assert!(ac.is_schedule_sub_active());
        assert!(
            !ac.is_active(),
            "primary popup should be off in schedule-sub mode"
        );
    }

    #[test]
    fn schedule_sub_confirm_appends_space_for_add_and_rm() {
        let mut ac = Autocomplete::new();
        ac.update("/schedule add");
        let text = ac.schedule_sub_confirm().expect("add match");
        assert_eq!(text, "/schedule add ");

        ac.update("/schedule rm");
        let text = ac.schedule_sub_confirm().expect("rm match");
        assert_eq!(text, "/schedule rm ");
    }

    #[test]
    fn schedule_sub_confirm_no_trailing_space_for_list() {
        let mut ac = Autocomplete::new();
        ac.update("/schedule list");
        let text = ac.schedule_sub_confirm().expect("list match");
        assert_eq!(text, "/schedule list");
    }

    #[test]
    fn all_subcommand_hint_modes_are_mutually_exclusive() {
        let mut ac = Autocomplete::new();
        ac.update("/op ");
        assert!(ac.is_op_sub_active());
        ac.update("/browser ");
        assert!(ac.is_browser_sub_active());
        assert!(!ac.is_op_sub_active());
        ac.update("/loop ");
        assert!(ac.is_loop_sub_active());
        assert!(!ac.is_browser_sub_active());
        ac.update("/schedule ");
        assert!(ac.is_schedule_sub_active());
        assert!(!ac.is_loop_sub_active());
    }
}
