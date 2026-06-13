//! Bottom status bar: mode, model, token counts, spinner, mode flags.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ready,
    Thinking,
    Streaming,
    Error,
}

pub struct StatusBar {
    pub mode: Mode,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub yolo: bool,
    pub sandbox: bool,
    spinner_frame: usize,
}

impl StatusBar {
    pub fn new(model: String) -> Self {
        Self {
            mode: Mode::Ready,
            model,
            input_tokens: 0,
            output_tokens: 0,
            yolo: false,
            sandbox: false,
            spinner_frame: 0,
        }
    }

    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let (label, color) = match self.mode {
            Mode::Ready => ("ready", theme.accent),
            Mode::Thinking => ("thinking", theme.system),
            Mode::Streaming => ("streaming", theme.accent),
            Mode::Error => ("error", Color::Red),
        };
        let frames = match self.mode {
            Mode::Streaming => &theme.spinner_streaming,
            _ => &theme.spinner_thinking,
        };
        let spin = if matches!(self.mode, Mode::Thinking | Mode::Streaming) && !frames.is_empty() {
            format!("{} ", frames[self.spinner_frame % frames.len()])
        } else {
            String::new()
        };

        let mut spans = vec![
            Span::styled(spin, Style::default().fg(color)),
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(self.model.clone(), Style::default().fg(theme.fg_text)),
            Span::raw("  "),
            Span::styled(
                format!("↑{} ↓{}", self.input_tokens, self.output_tokens),
                Style::default().fg(theme.fg_subtle),
            ),
        ];
        if self.yolo {
            spans.push(Span::styled("  YOLO", Style::default().fg(Color::Red)));
        }
        if self.sandbox {
            spans.push(Span::styled(
                "  SANDBOX",
                Style::default().fg(theme.accent_secondary),
            ));
        }

        let para = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg_secondary));
        f.render_widget(para, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_model_and_mode() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut sb = StatusBar::new("MiniMax-M1".into());
        sb.input_tokens = 12;
        let backend = TestBackend::new(60, 1);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| sb.render(f, f.area(), &theme)).unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("ready"));
        assert!(content.contains("MiniMax-M1"));
    }
}
