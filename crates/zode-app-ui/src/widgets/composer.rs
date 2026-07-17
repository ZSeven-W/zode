use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{components::text_area::TextArea, Painter, Point2D, Rect, TextLayout};
use zode_app_model::ComposerState;
use zode_node_protocol::{SandboxMode, UserContent};

use crate::{ImeEvent, Key, Modifiers, RectExt, ZodeTheme};

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

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.input.set_text(text.into());
        self.input.clear_composition();
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
            Key::PageUp | Key::PageDown | Key::Tab | Key::Escape | Key::Character(_) => {
                ComposerOutcome::Ignored
            }
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
        Self::paint_with_context(painter, rect, state, None, None, theme);
    }

    pub fn paint_with_branch(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ComposerState,
        branch: Option<&str>,
        theme: &ZodeTheme,
    ) {
        Self::paint_with_context(painter, rect, state, None, branch, theme);
    }

    pub fn paint_with_context(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ComposerState,
        connection_label: Option<&str>,
        branch: Option<&str>,
        theme: &ZodeTheme,
    ) {
        let input = TextInputState::with_text(state.draft.clone());
        Self::paint_input_with_context(
            painter,
            rect,
            &input,
            state,
            connection_label,
            branch,
            theme,
        );
    }

    pub fn paint_input(
        painter: &mut dyn Painter,
        rect: Rect,
        input: &TextInputState,
        state: &ComposerState,
        theme: &ZodeTheme,
    ) {
        Self::paint_input_with_context(painter, rect, input, state, None, None, theme);
    }

    pub fn paint_input_with_branch(
        painter: &mut dyn Painter,
        rect: Rect,
        input: &TextInputState,
        state: &ComposerState,
        branch: Option<&str>,
        theme: &ZodeTheme,
    ) {
        Self::paint_input_with_context(painter, rect, input, state, None, branch, theme);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_input_with_context(
        painter: &mut dyn Painter,
        rect: Rect,
        input: &TextInputState,
        state: &ComposerState,
        connection_label: Option<&str>,
        branch: Option<&str>,
        theme: &ZodeTheme,
    ) {
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

        let mut context_x = 16.0;
        for label in [Some("zode"), connection_label, branch]
            .into_iter()
            .flatten()
            .filter(|label| !label.trim().is_empty())
        {
            draw_text(
                painter,
                label,
                Point2D::new(rect.origin.x + context_x, rect.origin.y + 18.0),
                10.0,
                theme.tokens.muted_foreground,
            );
            context_x += painter.measure_text_weighted(label, 10.0, 400) + 20.0;
        }

        TextArea {
            state: input,
            placeholder: "向 Zode 描述一个任务",
            focused: state.focused,
            font_size: 14.0,
            now_ms: 0,
            pad_x: 8.0,
            max_visible_lines: 4,
        }
        .paint(
            painter,
            Rect::xywh(
                rect.origin.x + 8.0,
                rect.origin.y + 26.0,
                rect.size.x - 16.0,
                (rect.size.y - 70.0).max(0.0),
            ),
            &theme.tokens,
        );
        let controls_y = rect.origin.y + rect.size.y - 17.0;
        painter.stroke_svg_path(
            "M4 12H20M12 4V20",
            Point2D::new(rect.origin.x + 14.0, controls_y - 13.0),
            16.0,
            theme.tokens.muted_foreground,
            1.5,
        );
        if !state.sandbox_label.trim().is_empty() {
            draw_text(
                painter,
                &state.sandbox_label,
                Point2D::new(rect.origin.x + 44.0, controls_y),
                11.0,
                theme.tokens.muted_foreground,
            );
        }
        let model = state.model.as_deref().unwrap_or("选择模型");
        draw_text(
            painter,
            model,
            Point2D::new(
                (rect.max_x() - 190.0).max(rect.origin.x + 140.0),
                controls_y,
            ),
            11.0,
            theme.tokens.muted_foreground,
        );
        if let Some(effort) = state
            .effort
            .as_deref()
            .filter(|effort| !effort.trim().is_empty())
        {
            draw_text(
                painter,
                effort,
                Point2D::new(
                    (rect.max_x() - 108.0).max(rect.origin.x + 220.0),
                    controls_y,
                ),
                11.0,
                theme.tokens.muted_foreground,
            );
        }
        painter.stroke_svg_path(
            "M9 5V12A3 3 0 0 0 15 12V5M6 11A6 6 0 0 0 18 11M12 17V21",
            Point2D::new(rect.max_x() - 70.0, controls_y - 14.0),
            16.0,
            theme.tokens.muted_foreground,
            1.4,
        );
        let send = Rect::xywh(rect.max_x() - 42.0, rect.max_y() - 38.0, 28.0, 28.0);
        painter.fill_round_rect(send, 14.0, theme.zode_purple);
        painter.stroke_svg_path(
            "M7 13L12 8L17 13M12 8V18",
            Point2D::new(send.origin.x + 6.0, send.origin.y + 6.0),
            16.0,
            jian_widgets::Color::WHITE,
            1.6,
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
