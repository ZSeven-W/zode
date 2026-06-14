//! Main TUI layout: frame split and header rendering.

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

const TAB_RAIL_WIDTH: u16 = 36;
const MIN_WIDTH_FOR_TAB_RAIL: u16 = 96;

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

    let header_h = 1;
    let tabs_w = if show_tabs && area.height >= 8 && area.width >= MIN_WIDTH_FOR_TAB_RAIL {
        TAB_RAIL_WIDTH
    } else {
        0
    };
    let status_h = 1;
    let gutter_h = u16::from(area.height >= 8);
    let reserved_without_chat = header_h + status_h;
    let remaining = area.height.saturating_sub(reserved_without_chat);
    let composer_h = if area.height >= 8 {
        4.min(remaining.saturating_sub(1))
    } else {
        3.min(remaining.saturating_sub(1))
    };
    let chat_h = area
        .height
        .saturating_sub(header_h + composer_h + gutter_h + status_h)
        .max(1);

    let mut y = area.y;
    let header = Some(Rect::new(area.x, y, area.width, header_h));
    y = y.saturating_add(header_h);

    let content_x = area.x;
    let content_w = area.width.saturating_sub(tabs_w);
    let content_y = y;
    let content_h = area.height.saturating_sub(header_h);

    let tabs = if tabs_w > 0 {
        Some(Rect::new(
            area.x.saturating_add(content_w),
            content_y,
            tabs_w,
            content_h,
        ))
    } else {
        None
    };

    let chat = Rect::new(content_x, y, content_w, chat_h);
    y = y.saturating_add(chat_h);
    y = y.saturating_add(gutter_h);
    let composer = Rect::new(content_x, y, content_w, composer_h);
    y = y.saturating_add(composer_h);
    let status = Rect::new(content_x, y, content_w, status_h);

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
    let line = Line::from(vec![
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
        Span::styled("  /  ", Style::default().fg(theme.separator)),
        Span::styled(info.theme_name, Style::default().fg(theme.accent)),
        Span::styled("  /  ", Style::default().fg(theme.separator)),
        Span::styled(info.model, Style::default().fg(theme.fg_text)),
        Span::styled("  /  ", Style::default().fg(theme.separator)),
        Span::styled(compact_path(info.cwd), Style::default().fg(theme.fg_subtle)),
        Span::styled("  /  ", Style::default().fg(theme.separator)),
        Span::styled(info.tab_title, Style::default().fg(theme.fg_subtle)),
        Span::styled("  ·  ", Style::default().fg(theme.separator)),
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

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.bg_primary)),
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
    fn split_main_allocates_header_right_tabs_chat_composer_status() {
        let area = Rect::new(0, 0, 100, 30);
        let split = split_main(area, true);
        assert_eq!(split.header, Some(Rect::new(0, 0, 100, 1)));
        assert_eq!(split.tabs, Some(Rect::new(64, 1, 36, 29)));
        assert_eq!(split.chat, Rect::new(0, 1, 64, 23));
        assert_eq!(split.composer, Rect::new(0, 25, 64, 4));
        assert_eq!(split.status, Rect::new(0, 29, 64, 1));
    }

    #[test]
    fn split_main_leaves_gutter_between_chat_and_composer() {
        let area = Rect::new(0, 0, 100, 30);
        let split = split_main(area, true);
        let chat_bottom = split.chat.y.saturating_add(split.chat.height);
        assert_eq!(
            split.composer.y.saturating_sub(chat_bottom),
            1,
            "chat and composer should not touch"
        );
    }

    #[test]
    fn split_main_hides_right_tabs_when_width_is_tight() {
        let area = Rect::new(0, 0, 95, 30);
        let split = split_main(area, true);
        assert_eq!(split.tabs, None);
        assert_eq!(split.chat, Rect::new(0, 1, 95, 23));
        assert_eq!(split.composer, Rect::new(0, 25, 95, 4));
        assert_eq!(split.status, Rect::new(0, 29, 95, 1));
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
        assert!(text.contains("tab 1"));
        assert!(text.contains("running"));
    }

    #[test]
    fn header_uses_roomy_breadcrumb_spacing() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = TestBackend::new(120, 3);
        let mut term = Terminal::new(backend).unwrap();
        let cwd = Path::new("/Users/kayshen/Workspace/ZSeven-W/zode");
        term.draw(|f| {
            render_header(
                f,
                f.area(),
                &theme,
                HeaderInfo {
                    theme_name: &theme.name,
                    model: "deepseek-v4-pro",
                    cwd,
                    tab_title: "nihao",
                    busy: false,
                },
            )
        })
        .unwrap();

        let text = content(&term);
        assert!(text.contains("zode  /  Minimal  /  deepseek-v4-pro"));
        assert!(text.contains("nihao  ·  idle"));
    }
}
