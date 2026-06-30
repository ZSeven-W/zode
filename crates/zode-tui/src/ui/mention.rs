//! `@`-mention picker: type `@` in the input to reference a cwd file, a skill,
//! or an MCP server. Selecting inserts the bare reference (a relative path for
//! files; a name for skills / MCP servers) — lightweight, so the agent reads
//! the file with its Read tool / uses the skill or MCP on demand.
//!
//! The picker activates while the LAST whitespace-delimited token of the input
//! starts with `@`; the text after `@` filters the candidates.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::theme::Theme;

const MAX_VISIBLE: usize = 8;

/// Directories never descended into when collecting `@`-mention file candidates.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
];

/// Hard ceiling on directories visited, independent of the file cap, so a tree
/// that is broad-but-shallow-on-files can't stall the walk (the file cap alone
/// wouldn't bound traversal of many near-empty directories).
const MAX_DIRS: usize = 4000;

/// Collect repo-relative paths of files under `root`, skipping VCS/build/hidden
/// dirs, capped at `cap` files AND `MAX_DIRS` directories visited. A cheap
/// std-fs walk (no ignore-file parsing) — adequate for an interactive `@`
/// picker that filters in memory, and bounded so it never stalls the UI.
pub fn collect_cwd_files(root: &Path, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut dirs_visited = 0usize;
    while let Some(dir) = stack.pop() {
        if out.len() >= cap || dirs_visited >= MAX_DIRS {
            break;
        }
        dirs_visited += 1;
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue; // skip hidden files and dirs
            }
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_ref()) {
                    stack.push(entry.path());
                }
            } else if ft.is_file() {
                if let Ok(rel) = entry.path().strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                    if out.len() >= cap {
                        break;
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    File,
    Skill,
    Mcp,
}

impl MentionKind {
    fn glyph(self) -> &'static str {
        match self {
            Self::File => "▸",
            Self::Skill => "◆",
            Self::Mcp => "⚇",
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Skill => "skill",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MentionItem {
    /// What gets inserted into the input (a relative path, or a skill/MCP name).
    pub insert: String,
    /// Optional one-line description (e.g. a skill's summary); shown muted.
    pub detail: String,
    pub kind: MentionKind,
}

/// Return the active `@`-mention query: the text after `@` when the input's last
/// whitespace token starts with `@`. `None` when there's no in-progress mention
/// (so `email@x` or an already-completed `@foo bar` don't trigger it).
pub fn at_mention_query(text: &str) -> Option<&str> {
    let token = text.rsplit(|c: char| c.is_whitespace()).next()?;
    token.strip_prefix('@')
}

pub struct MentionPicker {
    all: Vec<MentionItem>,
    filtered: Vec<usize>,
    state: ListState,
}

impl MentionPicker {
    pub fn new(all: Vec<MentionItem>, query: &str) -> Self {
        let mut p = Self {
            all,
            filtered: Vec::new(),
            state: ListState::default(),
        };
        p.filter(query);
        p
    }

    /// Re-filter by the `@` query (case-insensitive substring on the insert text)
    /// and clamp the selection. Files that the query is a path-segment prefix of
    /// rank first, then other substring hits — keeps the obvious match on top.
    pub fn filter(&mut self, query: &str) {
        let q = query.to_ascii_lowercase();
        let mut hits: Vec<(u8, usize)> = self
            .all
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let hay = it.insert.to_ascii_lowercase();
                if q.is_empty() {
                    return Some((1, i));
                }
                if hay.starts_with(&q) || hay.rsplit('/').next().is_some_and(|s| s.starts_with(&q))
                {
                    Some((0, i)) // prefix / filename-prefix match ranks first
                } else if hay.contains(&q) {
                    Some((1, i))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by_key(|(rank, _)| *rank);
        self.filtered = hits.into_iter().map(|(_, i)| i).take(200).collect();
        let sel = if self.filtered.is_empty() {
            None
        } else {
            Some(
                self.state
                    .selected()
                    .unwrap_or(0)
                    .min(self.filtered.len() - 1),
            )
        };
        self.state.select(sel);
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub fn next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let i = self
            .state
            .selected()
            .map_or(0, |i| (i + 1) % self.filtered.len());
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len();
        let i = self.state.selected().map_or(0, |i| (i + n - 1) % n);
        self.state.select(Some(i));
    }

    /// The bare reference to insert for the highlighted item.
    pub fn selected_insert(&self) -> Option<&str> {
        let i = self.state.selected()?;
        let idx = *self.filtered.get(i)?;
        Some(self.all[idx].insert.as_str())
    }

    pub fn render(&mut self, f: &mut Frame, input_area: Rect, theme: &Theme) {
        if self.filtered.is_empty() {
            return;
        }
        let rows = self.filtered.len().min(MAX_VISIBLE) as u16;
        let area = popup_area(input_area, rows);
        let name_col = self
            .filtered
            .iter()
            .map(|&i| self.all[i].insert.chars().count())
            .max()
            .unwrap_or(0)
            .clamp(12, 48);
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .map(|&i| {
                let it = &self.all[i];
                let mut spans = vec![
                    Span::styled(
                        format!("  {} ", it.kind.glyph()),
                        Style::default().fg(theme.accent_secondary),
                    ),
                    Span::styled(
                        format!("{:<name_col$} ", it.insert),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                if !it.detail.is_empty() {
                    spans.push(Span::styled(
                        format!("{} ", it.detail),
                        Style::default().fg(theme.fg_text),
                    ));
                }
                spans.push(Span::styled(
                    format!("({})", it.kind.tag()),
                    Style::default().fg(theme.fg_subtle),
                ));
                ListItem::new(Line::from(spans))
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

/// Popup directly ABOVE the input box, exactly `rows` tall. The block only
/// draws LEFT|RIGHT borders, so no vertical rows are reserved for borders
/// (matches the slash-autocomplete popup).
fn popup_area(input_area: Rect, rows: u16) -> Rect {
    Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(rows),
        width: input_area.width,
        height: rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(insert: &str, kind: MentionKind) -> MentionItem {
        MentionItem {
            insert: insert.to_string(),
            detail: String::new(),
            kind,
        }
    }

    #[test]
    fn at_mention_query_only_on_trailing_at_token() {
        assert_eq!(at_mention_query("@src"), Some("src"));
        assert_eq!(at_mention_query("fix the @src/ma"), Some("src/ma"));
        assert_eq!(at_mention_query("@"), Some(""));
        // already-completed mention, or a bare email — not active.
        assert_eq!(at_mention_query("@foo bar"), None);
        assert_eq!(at_mention_query("a@b.com"), None);
        assert_eq!(at_mention_query("plain text"), None);
    }

    #[test]
    fn filter_ranks_prefix_then_substring_and_navigates() {
        let mut p = MentionPicker::new(
            vec![
                item("src/main.rs", MentionKind::File),
                item("src/app.rs", MentionKind::File),
                item("README.md", MentionKind::File),
                item("deepwiki", MentionKind::Mcp),
            ],
            "",
        );
        p.filter("src");
        // Both src/* files match; README/deepwiki filtered out.
        assert_eq!(p.filtered.len(), 2);
        assert!(p.selected_insert().unwrap().starts_with("src/"));
        p.next();
        assert!(p.selected_insert().unwrap().starts_with("src/"));

        p.filter("deep");
        assert_eq!(p.selected_insert(), Some("deepwiki"));

        p.filter("zzz");
        assert!(p.is_empty());
    }

    #[test]
    fn collect_cwd_files_skips_vcs_build_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("main.rs"), "").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/app.rs"), "").unwrap();
        // Skipped: build dir, and a hidden dir/file.
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target/big.o"), "").unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "").unwrap();
        std::fs::write(root.join(".env"), "").unwrap();

        let files = collect_cwd_files(root, 1000);
        assert!(files.contains(&"main.rs".to_string()));
        assert!(files.contains(&"src/app.rs".to_string()));
        assert!(!files.iter().any(|f| f.contains("target")));
        assert!(!files.iter().any(|f| f.contains(".git")));
        assert!(!files.contains(&".env".to_string()));
    }

    #[test]
    fn collect_cwd_files_honors_the_file_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..10 {
            std::fs::write(root.join(format!("f{i}.txt")), "").unwrap();
        }
        assert_eq!(collect_cwd_files(root, 3).len(), 3);
    }
}
