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
