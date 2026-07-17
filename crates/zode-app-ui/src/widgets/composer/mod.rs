use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{Painter, Rect};
use zode_app_model::{AttachmentMetadata, ComposerState, GoalProgress};
use zode_node_protocol::{SandboxMode, UserContent};

use crate::{
    stable_widget_id, ImeEvent, Key, Modifiers, RectExt, WidgetId, ZodeTheme,
    COMPOSER_ATTACHMENT_H, COMPOSER_CONTEXT_H, COMPOSER_INPUT_H,
};

mod attachments;
mod context;
mod input;

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerSubmission {
    pub content: Vec<UserContent>,
    /// Lightweight projection for the conversation UI. Endpoint dispatch uses only `content`.
    pub attachments: Vec<AttachmentMetadata>,
}

impl From<String> for ComposerSubmission {
    fn from(text: String) -> Self {
        Self {
            content: vec![UserContent::Text { text }],
            attachments: Vec::new(),
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
    AttachmentsChanged(Vec<AttachmentMetadata>),
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
    attachment_payloads: Vec<UserContent>,
    attachment_metadata: Vec<AttachmentMetadata>,
    next_attachment_id: u64,
    busy: bool,
    now_ms: u64,
}

impl ComposerController {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            input: TextInputState::with_text(text),
            attachment_payloads: Vec::new(),
            attachment_metadata: Vec::new(),
            next_attachment_id: 1,
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

    pub fn attachment_metadata(&self) -> &[AttachmentMetadata] {
        &self.attachment_metadata
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
        let mime_type = mime_type.into();
        let data_base64 = data_base64.into();
        let display_name = display_name.into();
        let byte_len = estimated_decoded_len(&data_base64);
        self.paste_image_with_metadata(
            mime_type.clone(),
            data_base64,
            AttachmentMetadata {
                id: String::new(),
                path: None,
                display_name,
                media_type: mime_type,
                width: None,
                height: None,
                byte_len,
            },
        )
    }

    pub fn paste_image_with_metadata(
        &mut self,
        mime_type: impl Into<String>,
        data_base64: impl Into<String>,
        mut metadata: AttachmentMetadata,
    ) -> ComposerOutcome {
        let mime_type = mime_type.into();
        metadata.id = format!("attachment-{}", self.next_attachment_id);
        self.next_attachment_id = self.next_attachment_id.saturating_add(1);
        metadata.media_type.clone_from(&mime_type);
        self.attachment_payloads.push(UserContent::Image {
            mime_type,
            data_base64: data_base64.into(),
            display_name: metadata.display_name.clone(),
        });
        self.attachment_metadata.push(metadata);
        ComposerOutcome::AttachmentsChanged(self.attachment_metadata.clone())
    }

    pub fn remove_attachment(&mut self, id: &str) -> ComposerOutcome {
        let Some(index) = self
            .attachment_metadata
            .iter()
            .position(|attachment| attachment.id == id)
        else {
            return ComposerOutcome::Ignored;
        };
        self.attachment_metadata.remove(index);
        self.attachment_payloads.remove(index);
        ComposerOutcome::AttachmentsChanged(self.attachment_metadata.clone())
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
        if self.input.text().trim().is_empty() && self.attachment_payloads.is_empty() {
            return ComposerOutcome::Ignored;
        }
        let mut content = Vec::with_capacity(1 + self.attachment_payloads.len());
        if !self.input.text().trim().is_empty() {
            content.push(UserContent::Text {
                text: self.input.text().to_owned(),
            });
        }
        content.append(&mut self.attachment_payloads);
        let attachments = std::mem::take(&mut self.attachment_metadata);
        self.input.set_text("");
        let submission = ComposerSubmission {
            content,
            attachments,
        };
        if self.busy {
            ComposerOutcome::Steer(submission)
        } else {
            ComposerOutcome::Send(submission)
        }
    }
}

fn estimated_decoded_len(data_base64: &str) -> u64 {
    let padding = data_base64
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    data_base64
        .len()
        .saturating_mul(3)
        .checked_div(4)
        .unwrap_or(0)
        .saturating_sub(padding) as u64
}

pub struct Composer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerLayout {
    pub context: Rect,
    pub attachments: Option<Rect>,
    pub input: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComposerAttachmentLayout {
    pub id: String,
    pub rect: Rect,
}

impl Composer {
    pub fn layout(rect: Rect, state: &ComposerState) -> ComposerLayout {
        let context_height = COMPOSER_CONTEXT_H.min(rect.size.y.max(0.0));
        let input_height = COMPOSER_INPUT_H.min((rect.size.y - context_height).max(0.0));
        let input = Rect::xywh(
            rect.origin.x,
            rect.max_y() - input_height,
            rect.size.x,
            input_height,
        );
        let attachments = (!state.attachments.is_empty()).then(|| {
            let available = (input.origin.y - rect.origin.y - context_height).max(0.0);
            Rect::xywh(
                rect.origin.x,
                rect.origin.y + context_height,
                rect.size.x,
                COMPOSER_ATTACHMENT_H.min(available),
            )
        });
        ComposerLayout {
            context: Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, context_height),
            attachments,
            input,
        }
    }

    pub(crate) fn attachment_layouts(
        rect: Rect,
        state: &ComposerState,
    ) -> Vec<ComposerAttachmentLayout> {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || state.attachments.is_empty() {
            return Vec::new();
        }
        let available = (rect.size.x - 24.0).max(0.0);
        let desired: f32 = 180.0;
        let gap: f32 = 8.0;
        state
            .attachments
            .iter()
            .enumerate()
            .scan(rect.origin.x + 12.0, |x, (index, attachment)| {
                let remaining = (rect.max_x() - 12.0 - *x).max(0.0);
                if remaining <= 0.0 {
                    return None;
                }
                let remaining_count = state.attachments.len() - index;
                let shared = ((available - gap * (remaining_count.saturating_sub(1) as f32))
                    / remaining_count as f32)
                    .max(0.0);
                let width = desired.min(shared).min(remaining);
                let item = ComposerAttachmentLayout {
                    id: attachment.id.clone(),
                    rect: Rect::xywh(
                        *x,
                        rect.origin.y + (rect.size.y - 32.0).max(0.0) / 2.0,
                        width,
                        32.0_f32.min(rect.size.y),
                    ),
                };
                *x += width + gap;
                Some(item)
            })
            .collect()
    }

    pub(crate) fn attachment_widget_id(id: &str) -> WidgetId {
        stable_widget_id(0x41, &id)
    }

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
        text_input: &TextInputState,
        state: &ComposerState,
        connection_label: Option<&str>,
        branch: Option<&str>,
        theme: &ZodeTheme,
    ) {
        Self::paint_input_with_workspace_context(
            painter,
            rect,
            text_input,
            state,
            None,
            connection_label,
            branch,
            None,
            theme,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_input_with_workspace_context(
        painter: &mut dyn Painter,
        rect: Rect,
        text_input: &TextInputState,
        state: &ComposerState,
        workspace_label: Option<&str>,
        connection_label: Option<&str>,
        branch: Option<&str>,
        goal: Option<&GoalProgress>,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(rect, state);
        context::paint(
            painter,
            layout.context,
            workspace_label,
            connection_label,
            branch,
            goal,
            theme,
        );
        if let Some(attachment_rect) = layout.attachments {
            let attachment_layouts = Self::attachment_layouts(attachment_rect, state);
            attachments::paint(
                painter,
                attachment_rect,
                &attachment_layouts,
                &state.attachments,
                theme,
            );
        }
        input::paint(painter, layout.input, text_input, state, theme);
    }
}
