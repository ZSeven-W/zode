//! Bottom status bar: mode, model, token counts, spinner, mode flags.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;
use zode_core::ui_extensions::{UiStatusLine, UiTone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ready,
    Thinking,
    Streaming,
    Compacting,
    Switching,
    Error,
}

pub struct StatusBar {
    pub mode: Mode,
    pub model: String,
    /// Provider-group name of the active model (e.g. "deepseek"). Shown as
    /// `model(provider)`; empty when the model isn't part of a configured group.
    pub provider: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Current context-window usage (tokens) and total, for the occupancy %.
    /// `context_window` of 0 hides the badge.
    pub context_tokens: u32,
    pub context_window: u32,
    pub yolo: bool,
    /// OS sandbox active for shell + file writes.
    pub sandbox: bool,
    /// Sandbox is in read-only mode (no writes at all).
    pub sandbox_read_only: bool,
    /// Outbound network is allowed inside the sandbox.
    pub sandbox_network: bool,
    pub plan_mode: bool,
    pub selection_mode: bool,
    spinner_frame: usize,
}

impl StatusBar {
    pub fn new(model: String) -> Self {
        Self {
            mode: Mode::Ready,
            model,
            provider: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 0,
            context_window: 0,
            yolo: false,
            sandbox: false,
            sandbox_read_only: false,
            sandbox_network: false,
            plan_mode: false,
            selection_mode: false,
            spinner_frame: 0,
        }
    }

    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// `model(provider)` when the provider group is known, else just `model`.
    fn model_label(&self) -> String {
        if self.provider.is_empty() {
            self.model.clone()
        } else {
            format!("{}({})", self.model, self.provider)
        }
    }

    /// Context-window occupancy as a 0–100 percentage, or `None` when the window
    /// size is unknown (no badge). Clamped so a slight over-count never exceeds 100.
    fn context_percent(&self) -> Option<u64> {
        if self.context_window == 0 {
            return None;
        }
        Some((self.context_tokens as u64 * 100 / self.context_window as u64).min(100))
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme, custom: &[UiStatusLine]) {
        let (label, color) = match self.mode {
            Mode::Ready => ("ready", theme.accent),
            Mode::Thinking => ("thinking", theme.system),
            Mode::Streaming => ("streaming", theme.accent),
            Mode::Compacting => ("compacting", theme.system),
            Mode::Switching => ("switching", theme.system),
            Mode::Error => ("error", Color::Red),
        };
        let frames = match self.mode {
            Mode::Streaming => &theme.spinner_streaming,
            _ => &theme.spinner_thinking,
        };
        let spin = if matches!(
            self.mode,
            Mode::Thinking | Mode::Streaming | Mode::Compacting | Mode::Switching
        ) && !frames.is_empty()
        {
            format!("{} ", frames[self.spinner_frame % frames.len()])
        } else {
            "● ".to_string()
        };

        let mut spans = vec![
            Span::styled(spin, Style::default().fg(color)),
            Span::styled(
                zode_core::i18n::t(label),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │ ", Style::default().fg(theme.separator)),
            // Active model, with its provider group in parens: `model(provider)`.
            Span::styled(self.model_label(), Style::default().fg(theme.fg_text)),
            Span::styled(" │ ", Style::default().fg(theme.separator)),
            Span::styled(
                format!("↑{} ↓{}", self.input_tokens, self.output_tokens),
                Style::default().fg(theme.fg_subtle),
            ),
        ];
        // Context-window occupancy, when known. Subtle until it gets tight.
        if let Some(pct) = self.context_percent() {
            let pct_color = if pct >= 90 {
                Color::Red
            } else if pct >= 75 {
                Color::Yellow
            } else {
                theme.fg_subtle
            };
            spans.push(Span::styled(" │ ", Style::default().fg(theme.separator)));
            spans.push(Span::styled(
                format!("{pct}% {}", crate::tr("ctx")),
                Style::default().fg(pct_color),
            ));
        }
        // Approval mode: YOLO (auto-approve, risky) in red, otherwise the safe
        // default is implicit (no badge).
        if self.yolo {
            spans.push(Span::styled(
                format!("  {}", crate::tr("YOLO")),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
        // Sandbox: always shown so the confinement state is visible. OFF is a
        // warning (writes unconfined); ON shows the mode + network.
        if self.sandbox {
            let label = if self.sandbox_read_only {
                format!("  {}", crate::tr("SANDBOX:RO"))
            } else {
                format!("  {}", crate::tr("SANDBOX"))
            };
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(theme.accent_secondary)
                    .add_modifier(Modifier::BOLD),
            ));
            if self.sandbox_network {
                spans.push(Span::styled(
                    format!(" +{}", crate::tr("NET")),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            spans.push(Span::styled(
                format!("  {}", crate::tr("UNSANDBOXED")),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
        if self.plan_mode {
            spans.push(Span::styled(
                format!("  {}", crate::tr("PLAN")),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if self.selection_mode {
            spans.push(Span::styled(
                format!("  {}", crate::tr("SELECT")),
                Style::default()
                    .fg(theme.accent_secondary)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        spans.extend([
            Span::styled(" │ ", Style::default().fg(theme.separator)),
            Span::styled(
                zode_core::i18n::t("F1 help"),
                Style::default().fg(theme.fg_subtle),
            ),
            Span::styled(
                format!(
                    " · {}",
                    zode_core::i18n::t("Ctrl+O settings")
                        .replace("Ctrl+", crate::primary_key_prefix())
                ),
                Style::default().fg(theme.fg_subtle),
            ),
            Span::styled(
                format!(
                    " · {}",
                    zode_core::i18n::t("Ctrl+B tasks")
                        .replace("Ctrl+", crate::primary_key_prefix())
                ),
                Style::default().fg(theme.fg_subtle),
            ),
        ]);

        let mut lines = vec![Line::from(spans)];
        if area.height > 1 && !custom.is_empty() {
            let mut custom_spans = Vec::new();
            for (index, line) in custom.iter().enumerate() {
                if index > 0 {
                    custom_spans.push(Span::styled(" │ ", Style::default().fg(theme.separator)));
                }
                custom_spans.extend(line.spans.iter().map(|span| {
                    let color = match span.tone.as_ref().unwrap_or(&UiTone::Default) {
                        UiTone::Default => theme.fg_text,
                        UiTone::Muted => theme.fg_subtle,
                        UiTone::Accent => theme.accent,
                        UiTone::Success => Color::Green,
                        UiTone::Warning => Color::Yellow,
                        UiTone::Danger => Color::Red,
                    };
                    let mut style = Style::default().fg(color);
                    if span.bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if span.italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    Span::styled(span.text.clone(), style)
                }));
            }
            lines.push(Line::from(custom_spans));
        }
        let para = Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary));
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
        term.draw(|f| sb.render(f, f.area(), &theme, &[])).unwrap();
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

    #[test]
    fn renders_flags_and_key_hints() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let mut sb = StatusBar::new("MiniMax-M1".into());
        sb.input_tokens = 10;
        sb.output_tokens = 20;
        sb.yolo = true;
        sb.sandbox = true;
        sb.mode = Mode::Streaming;
        sb.tick();
        let backend = TestBackend::new(100, 1);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| sb.render(f, f.area(), &theme, &[])).unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("streaming"));
        assert!(content.contains("MiniMax-M1"));
        assert!(content.contains("↑10 ↓20"));
        assert!(content.contains("YOLO"));
        assert!(content.contains("SANDBOX"));
        assert!(content.contains("F1 help"));
        assert!(content.contains("Ctrl+O settings"));
        assert!(content.contains("Ctrl+B tasks"));
        assert!(!content.contains("Cmd+"));
    }

    fn render_to_string(sb: &StatusBar) -> String {
        let theme = ThemeStore::with_builtins().resolve(None);
        let backend = TestBackend::new(120, 1);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| sb.render(f, f.area(), &theme, &[])).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn renders_provider_and_context_percent() {
        let mut sb = StatusBar::new("deepseek-v4-pro".into());
        sb.provider = "deepseek".into();
        sb.context_tokens = 250_000;
        sb.context_window = 1_000_000;
        let s = render_to_string(&sb);
        assert!(s.contains("deepseek-v4-pro(deepseek)"), "{s}");
        assert!(s.contains("25% ctx"), "{s}");
    }

    #[test]
    fn context_badge_hidden_without_window() {
        let sb = StatusBar::new("m".into()); // context_window defaults to 0
        assert!(!render_to_string(&sb).contains("ctx"));
    }

    #[test]
    fn renders_plugin_content_on_second_status_row() {
        use zode_core::ui_extensions::{UiSpan, UiStatusLine, UiTone};

        let theme = ThemeStore::with_builtins().resolve(None);
        let sb = StatusBar::new("m".into());
        let custom = vec![UiStatusLine {
            spans: vec![UiSpan {
                text: "plugin status".into(),
                tone: Some(UiTone::Accent),
                bold: true,
                italic: false,
            }],
        }];
        let backend = TestBackend::new(80, 2);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| sb.render(f, f.area(), &theme, &custom))
            .unwrap();
        let second_row: String = term.backend().buffer().content()[80..]
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(second_row.contains("plugin status"));
    }

    #[test]
    fn sandbox_badge_shows_off_mode_and_network() {
        let mut sb = StatusBar::new("m".into());
        // off → unconfined warning
        sb.sandbox = false;
        assert!(render_to_string(&sb).contains("UNSANDBOXED"));
        // read-only + network
        sb.sandbox = true;
        sb.sandbox_read_only = true;
        sb.sandbox_network = true;
        let s = render_to_string(&sb);
        assert!(s.contains("SANDBOX:RO"), "{s}");
        assert!(s.contains("+NET"), "{s}");
        // workspace-write, no network
        sb.sandbox_read_only = false;
        sb.sandbox_network = false;
        let s = render_to_string(&sb);
        assert!(s.contains("SANDBOX"));
        assert!(!s.contains(":RO"));
        assert!(!s.contains("+NET"));
    }
}
