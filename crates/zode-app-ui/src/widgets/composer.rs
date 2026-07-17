use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::ComposerState;
use zode_node_protocol::{SandboxMode, UserContent};

use crate::{ImeEvent, Key, Modifiers, ZodeTheme};

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerSubmission {
    pub content: Vec<UserContent>,
}

impl From<String> for ComposerSubmission {
    fn from(text: String) -> Self {
        Self {
            content: vec![UserContent::Text { text }],
        }
    }
}

impl From<&str> for ComposerSubmission {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxSelection {
    pub mode: SandboxMode,
    pub network: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComposerOutcome {
    Ignored,
    Edited,
    Send(ComposerSubmission),
    Steer(ComposerSubmission),
    Stop,
    SetModel(String),
    SetEffort(String),
    SetSandbox(SandboxSelection),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerController {
    input: TextInputState,
    attachments: Vec<UserContent>,
    busy: bool,
    now_ms: u64,
}

impl ComposerController {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            input: TextInputState::with_text(text),
            attachments: Vec::new(),
            busy: false,
            now_ms: 0,
        }
    }

    pub fn fixture(text: impl Into<String>) -> Self {
        Self::new(text)
    }

    pub fn text(&self) -> &str {
        self.input.text()
    }

    pub fn input_state(&self) -> &TextInputState {
        &self.input
    }

    pub fn composition_text(&self) -> Option<&str> {
        self.input
            .composition()
            .map(|composition| composition.text.as_str())
    }

    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> ComposerOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match key {
            Key::Enter if self.input.composition().is_some() => {
                self.input.commit_composition(self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Enter if modifiers.contains(Modifiers::SHIFT) => {
                self.input.insert_str("\n", self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Enter => self.submit(),
            Key::Backspace => {
                self.input.backspace(self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Delete => {
                self.input.delete_forward(self.now_ms);
                ComposerOutcome::Edited
            }
            Key::ArrowLeft => {
                self.input
                    .move_left(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ComposerOutcome::Edited
            }
            Key::ArrowRight => {
                self.input
                    .move_right(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Home => {
                self.input
                    .move_home(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ComposerOutcome::Edited
            }
            Key::End => {
                self.input
                    .move_end(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Character(character)
                if modifiers.primary() && character.eq_ignore_ascii_case("a") =>
            {
                self.input.select_all();
                ComposerOutcome::Edited
            }
            Key::Character(character)
                if !modifiers.primary() && !modifiers.contains(Modifiers::ALT) =>
            {
                self.input.insert_str(&character, self.now_ms);
                ComposerOutcome::Edited
            }
            Key::Tab | Key::Escape | Key::Character(_) => ComposerOutcome::Ignored,
        }
    }

    pub fn ime(&mut self, event: ImeEvent) -> ComposerOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match event {
            ImeEvent::Start => self.input.clear_composition(),
            ImeEvent::Update { text, cursor } => {
                let cursor = prev_char_boundary(&text, cursor.unwrap_or(text.len()));
                self.input.set_composition(text, cursor, self.now_ms);
            }
            ImeEvent::Commit(text) => {
                let cursor = text.len();
                self.input.set_composition(text, cursor, self.now_ms);
                self.input.commit_composition(self.now_ms);
            }
            ImeEvent::End => self.input.clear_composition(),
        }
        ComposerOutcome::Edited
    }

    pub fn paste_text(&mut self, text: &str) -> ComposerOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        self.input.insert_str(text, self.now_ms);
        ComposerOutcome::Edited
    }

    pub fn paste_image(
        &mut self,
        mime_type: impl Into<String>,
        data_base64: impl Into<String>,
        display_name: impl Into<String>,
    ) -> ComposerOutcome {
        self.attachments.push(UserContent::Image {
            mime_type: mime_type.into(),
            data_base64: data_base64.into(),
            display_name: display_name.into(),
        });
        ComposerOutcome::Edited
    }

    pub fn stop(&self) -> ComposerOutcome {
        if self.busy {
            ComposerOutcome::Stop
        } else {
            ComposerOutcome::Ignored
        }
    }

    pub fn select_model(&self, model: impl Into<String>) -> ComposerOutcome {
        ComposerOutcome::SetModel(model.into())
    }

    pub fn select_effort(&self, effort: impl Into<String>) -> ComposerOutcome {
        ComposerOutcome::SetEffort(effort.into())
    }

    pub fn select_sandbox(&self, sandbox: SandboxSelection) -> ComposerOutcome {
        ComposerOutcome::SetSandbox(sandbox)
    }

    fn submit(&mut self) -> ComposerOutcome {
        if self.input.text().trim().is_empty() && self.attachments.is_empty() {
            return ComposerOutcome::Ignored;
        }
        let mut content = Vec::with_capacity(1 + self.attachments.len());
        if !self.input.text().trim().is_empty() {
            content.push(UserContent::Text {
                text: self.input.text().to_owned(),
            });
        }
        content.append(&mut self.attachments);
        self.input.set_text("");
        let submission = ComposerSubmission { content };
        if self.busy {
            ComposerOutcome::Steer(submission)
        } else {
            ComposerOutcome::Send(submission)
        }
    }
}

pub struct Composer;

impl Composer {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ComposerState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.fill_drop_shadow(
            Rect::xywh(rect.origin.x, rect.origin.y + 2.0, rect.size.x, rect.size.y),
            12.0,
            18.0,
            theme.tokens.foreground.with_alpha(0.08),
        );
        painter.fill_round_rect(rect, 12.0, theme.tokens.card);
        painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);

        let prompt = if state.draft.is_empty() {
            "向 Zode 描述一个任务"
        } else {
            state.draft.as_str()
        };
        draw_text(
            painter,
            prompt,
            Point2D::new(rect.origin.x + 16.0, rect.origin.y + 30.0),
            14.0,
            if state.draft.is_empty() {
                theme.tokens.muted_foreground
            } else {
                theme.tokens.foreground
            },
        );
        let model = state.model.as_deref().unwrap_or("选择模型");
        draw_text(
            painter,
            model,
            Point2D::new(rect.origin.x + 16.0, rect.origin.y + rect.size.y - 16.0),
            11.0,
            theme.tokens.muted_foreground,
        );
        painter.fill_round_rect(
            Rect::xywh(
                rect.origin.x + rect.size.x - 42.0,
                rect.origin.y + rect.size.y - 38.0,
                28.0,
                28.0,
            ),
            14.0,
            theme.zode_purple,
        );
    }
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO);
    painter.draw_text(&layout, origin);
}
