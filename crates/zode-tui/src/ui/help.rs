//! Help overlay: two columns — slash commands (from CommandRegistry) and
//! keybindings (from KEYMAP). Dismissed with Esc.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use zode_core::commands::CommandRegistry;

use crate::keymap::KEYMAP;
use crate::theme::Theme;
use crate::ui::centered;

pub fn render_help(f: &mut Frame, area: Rect, theme: &Theme) {
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(theme.bg_primary)),
        area,
    );

    let popup = centered(area, 80, 70);
    f.render_widget(Clear, popup);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(popup);

    let reg = CommandRegistry::with_builtins();
    let cmd_lines: Vec<Line> = reg
        .all()
        .iter()
        .map(|c| Line::from(format!("/{:<10} {}", c.name, c.description)))
        .collect();
    let key_lines: Vec<Line> = KEYMAP
        .iter()
        .map(|b| Line::from(format!("{:<18} {}", b.keys, b.help)))
        .collect();

    let style = Style::default().bg(theme.bg_secondary).fg(theme.fg_text);
    let border = Style::default().fg(theme.accent);
    f.render_widget(
        Paragraph::new(cmd_lines).block(
            Block::default()
                .title(Line::styled(
                    " Commands ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(border)
                .style(style),
        ),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(key_lines).block(
            Block::default()
                .title(Line::styled(
                    " Keys (Esc to close) ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(border)
                .style(style),
        ),
        cols[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, widgets::Paragraph, Terminal};

    #[test]
    fn clears_the_full_screen_before_rendering_overlay() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            f.render_widget(Paragraph::new("LEAK_LEFT_SIDE"), f.area());
            render_help(f, f.area(), &theme);
        })
        .unwrap();

        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!content.contains("LEAK_LEFT_SIDE"));
        assert!(content.contains("Commands"));
        assert!(content.contains("Keys"));
    }
}
