use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{Painter, Rect};
use zode_app_model::{AppCommand, AttachmentMetadata, ComposerState, GoalProgress, ZodeAppState};
use zode_node_protocol::{SandboxMode, UserContent};

use crate::{stable_widget_id, ImeEvent, Key, Modifiers, RectExt, WidgetId, ZodeTheme};

mod attachments;
mod context;
mod footer;
mod ime;
mod input;
mod queue;
mod stack;

pub use context::{ComposerContextChipLayout, ComposerContextLayout};
pub use footer::{
    ComposerFooterLayout, ComposerFooterMenuLayout, ComposerFooterMenuWidget,
    ComposerFooterRowLayout, ComposerFooterSectionLayout, COMPOSER_ADD_FILE_ID,
    COMPOSER_ADD_GOAL_ID, COMPOSER_ADD_ID, COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID,
    COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_MODEL_ADD_ID, COMPOSER_MODEL_BACK_ID,
    COMPOSER_MODEL_CONFIGURE_ID, COMPOSER_MODEL_EFFORTS_ID, COMPOSER_MODEL_EFFORT_HIGH_ID,
    COMPOSER_MODEL_EFFORT_LOW_ID, COMPOSER_MODEL_EFFORT_MEDIUM_ID, COMPOSER_MODEL_EFFORT_XHIGH_ID,
    COMPOSER_MODEL_ID, COMPOSER_MODEL_MODELS_ID, COMPOSER_MODEL_RESET_ID, COMPOSER_MODEL_SPEEDS_ID,
    COMPOSER_MODEL_SPEED_ID, COMPOSER_PERMISSION_AUTO_ID, COMPOSER_PERMISSION_CUSTOM_ID,
    COMPOSER_PERMISSION_FULL_ID, COMPOSER_PERMISSION_ID, COMPOSER_PERMISSION_REQUEST_ID,
};
pub use queue::{ComposerQueueLayout, ComposerQueueMenuLayout, ComposerQueueRowLayout};
pub use stack::ComposerLayout;

pub const PROJECT_DETACH_ID: WidgetId = WidgetId(125);
pub const COMPOSER_PROJECT_ID: WidgetId = WidgetId(126);
pub const COMPOSER_LOCATION_ID: WidgetId = WidgetId(127);
pub const COMPOSER_BRANCH_ID: WidgetId = WidgetId(128);

const SELECT_PROJECT_LABEL: &str = "选择项目";

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
    Queue(ComposerSubmission),
    /// Priority send (Cmd/Ctrl+Enter) issued while a turn is running: the
    /// submission should join the head of the queue instead of the tail.
    QueueFront(ComposerSubmission),
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
    queue_edit_backup: Option<QueueEditBackup>,
}

#[derive(Debug, Clone, PartialEq)]
struct QueueEditBackup {
    input: TextInputState,
    attachment_payloads: Vec<UserContent>,
    attachment_metadata: Vec<AttachmentMetadata>,
    target_has_attachments: bool,
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
            queue_edit_backup: None,
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

    /// Temporarily edits one queued message without destroying the current draft.
    /// Queue editing is text-only; pending attachments are restored on finish.
    pub fn begin_queue_edit(&mut self, text: impl Into<String>) -> bool {
        self.begin_queue_edit_for_message(text, false)
    }

    /// Starts or retargets queue editing while retaining the original draft backup.
    pub fn begin_queue_edit_for_message(
        &mut self,
        text: impl Into<String>,
        target_has_attachments: bool,
    ) -> bool {
        let input = TextInputState::with_text(text.into());
        if let Some(backup) = self.queue_edit_backup.as_mut() {
            self.input = input;
            backup.target_has_attachments = target_has_attachments;
            return true;
        }
        self.queue_edit_backup = Some(QueueEditBackup {
            input: std::mem::replace(&mut self.input, input),
            attachment_payloads: std::mem::take(&mut self.attachment_payloads),
            attachment_metadata: std::mem::take(&mut self.attachment_metadata),
            target_has_attachments,
        });
        true
    }

    /// Leaves queue-edit mode and restores the exact draft and attachments that
    /// were present before editing began.
    pub fn finish_queue_edit(&mut self) -> bool {
        let Some(backup) = self.queue_edit_backup.take() else {
            return false;
        };
        self.input = backup.input;
        self.attachment_payloads = backup.attachment_payloads;
        self.attachment_metadata = backup.attachment_metadata;
        true
    }

    pub fn queue_editing(&self) -> bool {
        self.queue_edit_backup.is_some()
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
            // Priority send: joins the head of the queue instead of the tail
            // while a turn is running. With no turn running there is nothing
            // to jump ahead of, so it behaves like a plain Enter.
            Key::Enter if modifiers.primary() => self.submit_with_priority(true),
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
            Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Tab
            | Key::Escape
            | Key::Character(_) => ComposerOutcome::Ignored,
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
        if self.queue_edit_backup.is_some() {
            return ComposerOutcome::Ignored;
        }
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
        self.submit_with_priority(false)
    }

    /// `front` requests priority send (join the queue head). It only changes
    /// behavior while busy; an idle submit always sends directly regardless
    /// of `front`, since there is no running turn's queue to jump ahead of.
    fn submit_with_priority(&mut self, front: bool) -> ComposerOutcome {
        let queued_attachments_remain = self
            .queue_edit_backup
            .as_ref()
            .is_some_and(|backup| backup.target_has_attachments);
        if self.input.text().trim().is_empty()
            && self.attachment_payloads.is_empty()
            && !queued_attachments_remain
        {
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
        if !self.busy {
            ComposerOutcome::Send(submission)
        } else if front {
            ComposerOutcome::QueueFront(submission)
        } else {
            ComposerOutcome::Queue(submission)
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComposerAttachmentLayout {
    pub id: String,
    pub rect: Rect,
}

impl Composer {
    pub fn can_submit(state: &ComposerState) -> bool {
        !state.draft.trim().is_empty() || !state.attachments.is_empty()
    }

    pub fn layout(rect: Rect, state: &ComposerState) -> ComposerLayout {
        Self::layout_with_queue_count(rect, state, 0)
    }

    pub fn layout_for_state(rect: Rect, state: &ZodeAppState) -> ComposerLayout {
        let queue_count = queue::current_queue(state).map_or(0, |(_, items)| items.len());
        Self::layout_with_queue_count(rect, &state.composer, queue_count)
    }

    /// Returns the logical caret rectangle used to anchor the native IME
    /// candidate window. Geometry is probed through the same `TextArea` paint
    /// path as the visible composer so wrapping, font metrics and preedit stay aligned.
    pub fn ime_cursor_area(
        painter: &mut dyn Painter,
        rect: Rect,
        text_input: &TextInputState,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> Option<Rect> {
        ime::cursor_area(
            painter,
            Self::layout_for_state(rect, state).input,
            text_input,
            theme,
        )
    }

    fn layout_with_queue_count(
        rect: Rect,
        state: &ComposerState,
        queue_count: usize,
    ) -> ComposerLayout {
        stack::layout(rect, state, queue_count)
    }

    pub fn queue_layout(rect: Rect, state: &ZodeAppState) -> Option<ComposerQueueLayout> {
        Self::queue_layout_in_viewport(rect, rect, state)
    }

    pub(crate) fn queue_layout_in_viewport(
        rect: Rect,
        viewport: Rect,
        state: &ZodeAppState,
    ) -> Option<ComposerQueueLayout> {
        let slot = Self::layout_for_state(rect, state).queue?;
        queue::layout(slot, viewport, state)
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == PROJECT_DETACH_ID
            && state.current_session.is_none()
            && state.active_available_workspace().is_some()
        {
            return Some(AppCommand::BeginTask {
                workspace_uri: None,
            });
        }
        queue::command_for_widget(state, id)
    }

    pub fn context_layout(
        rect: Rect,
        state: &ZodeAppState,
        workspace_label: Option<&str>,
    ) -> ComposerContextLayout {
        Self::context_interaction_layout(rect, state, workspace_label, None, None)
    }

    pub fn context_interaction_layout(
        rect: Rect,
        state: &ZodeAppState,
        workspace_label: Option<&str>,
        connection_label: Option<&str>,
        branch: Option<&str>,
    ) -> ComposerContextLayout {
        let context = Self::layout_for_state(rect, state).context;
        let project_label = Self::context_project_label(state, workspace_label);
        context::layout(
            context,
            project_label,
            connection_label,
            branch,
            state.current_session.is_none(),
            state.current_session.is_none() && workspace_label.is_some(),
        )
    }

    pub(crate) fn context_project_label<'a>(
        state: &ZodeAppState,
        workspace_label: Option<&'a str>,
    ) -> Option<&'a str> {
        workspace_label.or_else(|| {
            state
                .current_session
                .is_none()
                .then_some(SELECT_PROJECT_LABEL)
        })
    }

    pub(crate) fn attachment_layouts(
        rect: Rect,
        state: &ComposerState,
    ) -> Vec<ComposerAttachmentLayout> {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 || state.attachments.is_empty() {
            return Vec::new();
        }
        let available = (rect.size.x - 24.0).max(0.0);
        let desired: f32 = 240.0;
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
        input::paint_surface(painter, layout.input, text_input, state, theme);
        let mut preview_state = zode_app_model::demo_state();
        preview_state.composer = state.clone();
        let footer = ComposerFooterMenuWidget::trigger_layout(layout.input, &preview_state);
        footer::ComposerFooterMenuWidget::paint_controls(
            painter,
            footer,
            &preview_state,
            Self::can_submit(state),
            false,
            None,
            None,
            theme,
        );
    }

    pub fn paint_input_with_app_context(
        painter: &mut dyn Painter,
        rect: Rect,
        text_input: &TextInputState,
        state: &ZodeAppState,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        Self::paint_input_with_workspace_app_context(
            painter, rect, text_input, state, None, None, None, None, None, hovered, theme,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_input_with_workspace_app_context(
        painter: &mut dyn Painter,
        rect: Rect,
        text_input: &TextInputState,
        state: &ZodeAppState,
        workspace_label: Option<&str>,
        connection_label: Option<&str>,
        branch: Option<&str>,
        goal: Option<&GoalProgress>,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        Self::paint_input_with_workspace_app_context_interactions(
            painter,
            rect,
            text_input,
            state,
            workspace_label,
            connection_label,
            branch,
            goal,
            focused,
            hovered,
            None,
            theme,
        );
    }

    /// Paints the app composer with explicit menu ownership for its context chips.
    /// `menu_active` keeps the trigger pill highlighted while its popover owns focus.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_input_with_workspace_app_context_interactions(
        painter: &mut dyn Painter,
        rect: Rect,
        text_input: &TextInputState,
        state: &ZodeAppState,
        workspace_label: Option<&str>,
        connection_label: Option<&str>,
        branch: Option<&str>,
        goal: Option<&GoalProgress>,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        menu_active: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout_for_state(rect, state);
        let project_label = Self::context_project_label(state, workspace_label);
        context::paint_interactive(
            painter,
            layout.context,
            project_label,
            connection_label,
            branch,
            goal,
            state.current_session.is_none(),
            state.current_session.is_none() && workspace_label.is_some(),
            focused,
            hovered,
            menu_active,
            theme,
        );
        if let Some(attachment_rect) = layout.attachments {
            let attachment_layouts = Self::attachment_layouts(attachment_rect, &state.composer);
            attachments::paint(
                painter,
                attachment_rect,
                &attachment_layouts,
                &state.composer.attachments,
                theme,
            );
        }
        let queue_layout = Self::queue_layout(rect, state);
        if let (Some(queue_layout), Some((_, messages))) =
            (queue_layout.as_ref(), queue::current_queue(state))
        {
            queue::paint_rows(painter, queue_layout, messages, focused, hovered, theme);
        }
        input::paint_surface(painter, layout.input, text_input, &state.composer, theme);
        footer::ComposerFooterMenuWidget::paint_controls(
            painter,
            footer::ComposerFooterMenuWidget::trigger_layout(layout.input, state),
            state,
            Self::can_submit(&state.composer),
            current_session_busy(state),
            focused,
            hovered,
            theme,
        );
        if let Some(queue_layout) = queue_layout.as_ref() {
            queue::paint_overlays(painter, queue_layout, focused, hovered, theme);
        }
    }
}

fn current_session_busy(state: &ZodeAppState) -> bool {
    state
        .current_session
        .as_ref()
        .and_then(|session| state.transcripts.get(session))
        .is_some_and(|transcript| transcript.busy)
}
