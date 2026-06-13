//! Main TUI chrome: frame split and header rendering.

use std::path::{Component, Path};

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct ChromeAreas {
    pub header: Option<Rect>,
    pub tabs: Option<Rect>,
    pub chat: Rect,
    pub composer: Rect,
    pub status: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct HeaderInfo<'a> {
    pub theme_name: &'a str,
    pub model: &'a str,
    pub cwd: &'a Path,
    pub tab_title: &'a str,
    pub busy: bool,
}

pub fn split_main(area: Rect, show_tabs: bool) -> ChromeAreas {
    if area.height <= 1 {
        return ChromeAreas {
            header: None,
            tabs: None,
            chat: area,
            composer: Rect::new(area.x, area.y.saturating_add(area.height), area.width, 0),
            status: Rect::new(area.x, area.y.saturating_add(area.height), area.width, 0),
        };
    }

    let header_h = if area.height >= 7 { 2 } else { 1 };
    let tabs_h = u16::from(show_tabs && area.height >= 8);
    let status_h = 1;
    let reserved_without_chat = header_h + tabs_h + status_h;
    let remaining = area.height.saturating_sub(reserved_without_chat);
    let composer_h = if area.height >= 8 {
        4.min(remaining.saturating_sub(1))
    } else {
        3.min(remaining.saturating_sub(1))
    };
    let chat_h = area
        .height
        .saturating_sub(header_h + tabs_h + composer_h + status_h)
        .max(1);

    let mut y = area.y;
    let header = Some(Rect::new(area.x, y, area.width, header_h));
    y = y.saturating_add(header_h);

    let tabs = if tabs_h > 0 {
        let rect = Rect::new(area.x, y, area.width, tabs_h);
        y = y.saturating_add(tabs_h);
        Some(rect)
    } else {
        None
    };

    let chat = Rect::new(area.x, y, area.width, chat_h);
    y = y.saturating_add(chat_h);
    let composer = Rect::new(area.x, y, area.width, composer_h);
    y = y.saturating_add(composer_h);
    let status = Rect::new(area.x, y, area.width, status_h);

    ChromeAreas {
        header,
        tabs,
        chat,
        composer,
        status,
    }
}

pub fn compact_path(cwd: &Path) -> String {
    let parts: Vec<String> = cwd
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    match parts.len() {
        0 => cwd.display().to_string(),
        1 => parts[0].clone(),
        n => format!(".../{}/{}", parts[n - 2], parts[n - 1]),
    }
}

pub fn render_header(f: &mut Frame, area: Rect, theme: &Theme, info: HeaderInfo<'_>) {
    if area.is_empty() {
        return;
    }

    let state = if info.busy { "running" } else { "idle" };
    let title = Line::from(vec![
        Span::styled(
            format!("{} ", theme.icon_logo),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "zode",
            Style::default()
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::default().fg(theme.separator)),
        Span::styled(info.theme_name, Style::default().fg(theme.accent)),
        Span::styled(" / ", Style::default().fg(theme.separator)),
        Span::styled(info.model, Style::default().fg(theme.fg_text)),
        Span::styled(" / ", Style::default().fg(theme.separator)),
        Span::styled(compact_path(info.cwd), Style::default().fg(theme.fg_subtle)),
    ]);
    let meta = Line::from(vec![
        Span::styled(info.tab_title, Style::default().fg(theme.fg_subtle)),
        Span::raw("  "),
        Span::styled(
            state,
            Style::default()
                .fg(if info.busy {
                    theme.accent_secondary
                } else {
                    theme.fg_subtle
                })
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let lines = if area.height > 1 {
        vec![title, meta]
    } else {
        vec![Line::from(vec![
            Span::styled(
                format!("{} zode", theme.icon_logo),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / ", Style::default().fg(theme.separator)),
            Span::styled(info.theme_name, Style::default().fg(theme.accent)),
            Span::styled(" / ", Style::default().fg(theme.separator)),
            Span::styled(info.model, Style::default().fg(theme.fg_text)),
            Span::styled(" / ", Style::default().fg(theme.separator)),
            Span::styled(state, Style::default().fg(theme.accent_secondary)),
        ])]
    };

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.bg_primary)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, Terminal};

    fn content(term: &Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn split_main_allocates_header_tabs_chat_composer_status() {
        let area = Rect::new(0, 0, 100, 30);
        let split = split_main(area, true);
        assert_eq!(split.header, Some(Rect::new(0, 0, 100, 2)));
        assert_eq!(split.tabs, Some(Rect::new(0, 2, 100, 1)));
        assert_eq!(split.chat, Rect::new(0, 3, 100, 22));
        assert_eq!(split.composer, Rect::new(0, 25, 100, 4));
        assert_eq!(split.status, Rect::new(0, 29, 100, 1));
    }

    #[test]
    fn split_main_preserves_core_areas_on_short_terminals() {
        let area = Rect::new(0, 0, 80, 6);
        let split = split_main(area, false);
        assert_eq!(split.header, Some(Rect::new(0, 0, 80, 1)));
        assert_eq!(split.tabs, None);
        assert_eq!(split.chat.height, 1);
        assert_eq!(split.composer.height, 3);
        assert_eq!(split.status.height, 1);
    }

    #[test]
    fn compact_path_keeps_last_two_components() {
        let path = Path::new("/Users/kayshen/Workspace/ZSeven-W/zode");
        assert_eq!(compact_path(path), ".../ZSeven-W/zode");
    }

    #[test]
    fn header_renders_brand_theme_model_and_cwd() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = TestBackend::new(100, 3);
        let mut term = Terminal::new(backend).unwrap();
        let cwd = Path::new("/Users/kayshen/Workspace/ZSeven-W/zode");
        term.draw(|f| {
            render_header(
                f,
                f.area(),
                &theme,
                HeaderInfo {
                    theme_name: &theme.name,
                    model: "MiniMax-M1",
                    cwd,
                    tab_title: "tab 1",
                    busy: true,
                },
            )
        })
        .unwrap();
        let text = content(&term);
        assert!(text.contains("zode"));
        assert!(text.contains("Cyberpunk"));
        assert!(text.contains("MiniMax-M1"));
        assert!(text.contains("ZSeven-W/zode"));
        assert!(text.contains("running"));
    }
}
