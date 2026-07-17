use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{
    components::text_input::TextInputView, Color, HorizontalAlign, Painter, Point2D, Rect,
    TextLayout, TextMetrics,
};
use zode_app_model::{AppCommand, SettingsCategory, ShellRoute, ZodeAppState};
use zode_node_protocol::SessionLocator;

use super::project_sidebar::workspace_label;
use crate::{
    paint_elevated_surface, paint_single_line, stable_widget_id, ImeEvent, Key, MenuRow, Modifiers,
    RectExt, SemanticIcon, WidgetId, ZodeTheme,
};

pub const GLOBAL_SEARCH_SURFACE_ID: WidgetId = WidgetId(220);
pub const GLOBAL_SEARCH_INPUT_ID: WidgetId = WidgetId(221);
pub const GLOBAL_SEARCH_NEW_TASK_ID: WidgetId = WidgetId(222);
pub const GLOBAL_SEARCH_OPEN_FOLDER_ID: WidgetId = WidgetId(223);
pub const GLOBAL_SEARCH_SETTINGS_ID: WidgetId = WidgetId(224);
pub const GLOBAL_SEARCH_SCRIM_ID: WidgetId = WidgetId(225);

const THREAD_ROW_NAMESPACE: u8 = 0x75;
const EDGE_INSET: f32 = 24.0;
const SURFACE_W: f32 = 560.0;
const SURFACE_PAD_X: f32 = 12.0;
const SURFACE_PAD_Y: f32 = 10.0;
const SEARCH_H: f32 = 42.0;
const SEARCH_GAP: f32 = 8.0;
const THREAD_ROW_H: f32 = 46.0;
const ACTION_ROW_H: f32 = 40.0;
const SEPARATOR_H: f32 = 1.0;
const MAX_VISIBLE_THREADS: usize = 6;
const ROW_PAD_X: f32 = 12.0;
const ICON_SIZE: f32 = 16.0;
const ICON_GAP: f32 = 10.0;
const MIN_VIEWPORT_H: f32 = 240.0;
const MIN_SURFACE_H: f32 =
    SURFACE_PAD_Y * 2.0 + SEARCH_H + SEARCH_GAP + THREAD_ROW_H + SEPARATOR_H + 3.0 * ACTION_ROW_H;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalSearchViewState {
    pub open: bool,
    pub query: String,
    pub active_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalSearchOutcome {
    Ignored,
    Edited,
}

/// Host-owned editable buffer for the global search field.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSearchController {
    input: TextInputState,
    now_ms: u64,
}

impl GlobalSearchController {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            input: TextInputState::with_text(text.into()),
            now_ms: 0,
        }
    }

    pub fn input_state(&self) -> &TextInputState {
        &self.input
    }

    pub fn text(&self) -> &str {
        self.input.text()
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.input.set_text(text.into());
        self.input.clear_composition();
    }

    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> GlobalSearchOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match key {
            Key::Enter if self.input.composition().is_some() => {
                self.input.commit_composition(self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::Backspace => {
                self.input.backspace(self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::Delete => {
                self.input.delete_forward(self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::ArrowLeft => {
                self.input
                    .move_left(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::ArrowRight => {
                self.input
                    .move_right(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::Home => {
                self.input
                    .move_home(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::End => {
                self.input
                    .move_end(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::Character(character)
                if modifiers.primary() && character.eq_ignore_ascii_case("a") =>
            {
                self.input.select_all();
                GlobalSearchOutcome::Edited
            }
            Key::Character(character)
                if !modifiers.primary() && !modifiers.contains(Modifiers::ALT) =>
            {
                self.input.insert_str(&character, self.now_ms);
                GlobalSearchOutcome::Edited
            }
            Key::Enter
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Tab
            | Key::Escape
            | Key::Character(_) => GlobalSearchOutcome::Ignored,
        }
    }

    pub fn ime(&mut self, event: ImeEvent) -> GlobalSearchOutcome {
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
        GlobalSearchOutcome::Edited
    }

    pub fn paste_text(&mut self, text: &str) -> GlobalSearchOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        self.input.insert_str(text, self.now_ms);
        GlobalSearchOutcome::Edited
    }
}

impl Default for GlobalSearchController {
    fn default() -> Self {
        Self::new("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalSearchTarget {
    Thread(SessionLocator),
    NewTask,
    OpenFolder,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSearchChoice {
    pub id: WidgetId,
    pub label: String,
    pub detail: Option<String>,
    pub target: GlobalSearchTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSearchRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub detail: Option<String>,
    pub target: GlobalSearchTarget,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSearchLayout {
    pub scrim: Rect,
    pub surface: Rect,
    pub search: Rect,
    pub thread_rows: Vec<GlobalSearchRowLayout>,
    pub empty_results: Option<Rect>,
    pub separator: Rect,
    pub action_rows: Vec<GlobalSearchRowLayout>,
}

pub struct GlobalSearch;

impl GlobalSearch {
    pub fn thread_widget_id(session: &SessionLocator) -> WidgetId {
        stable_widget_id(THREAD_ROW_NAMESPACE, session)
    }

    /// Matching, non-archived tasks in deterministic recency order.
    pub fn choices(state: &ZodeAppState, query: &str) -> Vec<GlobalSearchChoice> {
        let query = query.trim().to_lowercase();
        let mut threads = state
            .threads
            .iter()
            .filter(|thread| !state.archived_sessions.contains(&thread.session))
            .filter(|thread| {
                if query.is_empty() {
                    return true;
                }
                let workspace = workspace_label(&thread.workspace_uri, true).to_lowercase();
                thread.title.to_lowercase().contains(&query)
                    || workspace.contains(&query)
                    || thread.session.session_id.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.session.cmp(&right.session))
        });
        threads
            .into_iter()
            .take(MAX_VISIBLE_THREADS)
            .map(|thread| GlobalSearchChoice {
                id: Self::thread_widget_id(&thread.session),
                label: thread.title.clone(),
                detail: Some(workspace_label(&thread.workspace_uri, true)),
                target: GlobalSearchTarget::Thread(thread.session.clone()),
            })
            .collect()
    }

    /// Selectable targets in the exact order represented by this visible
    /// layout. Hidden task matches are deliberately absent.
    pub fn selectable_ids(layout: &GlobalSearchLayout) -> Vec<WidgetId> {
        layout
            .thread_rows
            .iter()
            .chain(layout.action_rows.iter())
            .map(|row| row.id)
            .collect()
    }

    pub fn command_for_active(
        state: &ZodeAppState,
        view: &GlobalSearchViewState,
        layout: &GlobalSearchLayout,
    ) -> Option<AppCommand> {
        let ids = Self::selectable_ids(layout);
        let active_index = normalized_active_index(view.active_index, ids.len())?;
        let id = ids[active_index];
        Self::command_for_widget(state, id)
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == GLOBAL_SEARCH_SCRIM_ID {
            return Some(AppCommand::CloseGlobalSearch);
        }
        if id == GLOBAL_SEARCH_NEW_TASK_ID {
            return Some(AppCommand::BeginTask {
                workspace_uri: state.active_available_workspace().cloned(),
            });
        }
        if id == GLOBAL_SEARCH_OPEN_FOLDER_ID {
            return Some(AppCommand::CreateProject);
        }
        if id == GLOBAL_SEARCH_SETTINGS_ID {
            return Some(AppCommand::Navigate(ShellRoute::Settings(
                SettingsCategory::General,
            )));
        }
        state
            .threads
            .iter()
            .find(|thread| {
                !state.archived_sessions.contains(&thread.session)
                    && Self::thread_widget_id(&thread.session) == id
            })
            .map(|thread| AppCommand::SelectSession(thread.session.clone()))
    }

    pub fn layout(
        viewport: Rect,
        state: &ZodeAppState,
        view: &GlobalSearchViewState,
    ) -> Option<GlobalSearchLayout> {
        if !view.open || viewport.size.x <= 0.0 || viewport.size.y < MIN_VIEWPORT_H {
            return None;
        }
        let width = SURFACE_W.min((viewport.size.x - EDGE_INSET * 2.0).max(0.0));
        if width < 240.0 || viewport.size.y < 240.0 {
            return None;
        }
        let choices = Self::choices(state, &view.query);
        let thread_count = choices.len().max(1);
        let desired_height = SURFACE_PAD_Y * 2.0
            + SEARCH_H
            + SEARCH_GAP
            + thread_count as f32 * THREAD_ROW_H
            + SEPARATOR_H
            + 3.0 * ACTION_ROW_H;
        // Preserve the generous 24 px inset when there is room, then relax it
        // near the minimum window height so one task and all command rows stay
        // visible and reachable.
        let max_height = (viewport.size.y - EDGE_INSET * 2.0)
            .max(MIN_SURFACE_H)
            .min(viewport.size.y);
        let height = desired_height.min(max_height);
        let surface = Rect::xywh(
            viewport.origin.x + (viewport.size.x - width) / 2.0,
            viewport.origin.y + (viewport.size.y - height) / 2.0,
            width,
            height,
        );
        let content_x = surface.origin.x + SURFACE_PAD_X;
        let content_w = (surface.size.x - SURFACE_PAD_X * 2.0).max(0.0);
        let search = Rect::xywh(
            content_x,
            surface.origin.y + SURFACE_PAD_Y,
            content_w,
            SEARCH_H,
        );
        let rows_y = search.max_y() + SEARCH_GAP;
        let max_thread_bottom = surface.max_y() - SURFACE_PAD_Y - 3.0 * ACTION_ROW_H - SEPARATOR_H;
        let visible_slots = ((max_thread_bottom - rows_y) / THREAD_ROW_H)
            .floor()
            .max(1.0) as usize;
        let visible_choices = choices.into_iter().take(visible_slots).collect::<Vec<_>>();
        let thread_rows = visible_choices
            .iter()
            .enumerate()
            .map(|(index, choice)| GlobalSearchRowLayout {
                id: choice.id,
                rect: Rect::xywh(
                    content_x,
                    rows_y + index as f32 * THREAD_ROW_H,
                    content_w,
                    THREAD_ROW_H,
                ),
                label: choice.label.clone(),
                detail: choice.detail.clone(),
                target: choice.target.clone(),
                active: view.active_index == index,
            })
            .collect::<Vec<_>>();
        let empty_results = thread_rows.is_empty().then_some(Rect::xywh(
            content_x,
            rows_y,
            content_w,
            THREAD_ROW_H,
        ));
        let thread_bottom = thread_rows
            .last()
            .map_or(rows_y + THREAD_ROW_H, |row| row.rect.max_y());
        let separator = Rect::xywh(content_x, thread_bottom, content_w, SEPARATOR_H);
        let actions = [
            (
                GLOBAL_SEARCH_NEW_TASK_ID,
                "新建任务",
                GlobalSearchTarget::NewTask,
            ),
            (
                GLOBAL_SEARCH_OPEN_FOLDER_ID,
                "打开文件夹",
                GlobalSearchTarget::OpenFolder,
            ),
            (
                GLOBAL_SEARCH_SETTINGS_ID,
                "设置",
                GlobalSearchTarget::Settings,
            ),
        ];
        let choice_count = visible_choices.len();
        let active_index = normalized_active_index(view.active_index, choice_count + actions.len());
        let action_rows = actions
            .into_iter()
            .enumerate()
            .map(
                |(action_index, (id, label, target))| GlobalSearchRowLayout {
                    id,
                    rect: Rect::xywh(
                        content_x,
                        separator.max_y() + action_index as f32 * ACTION_ROW_H,
                        content_w,
                        ACTION_ROW_H,
                    ),
                    label: label.into(),
                    detail: None,
                    target,
                    active: active_index == Some(choice_count + action_index),
                },
            )
            .collect();
        let mut thread_rows = thread_rows;
        for (index, row) in thread_rows.iter_mut().enumerate() {
            row.active = active_index == Some(index);
        }
        Some(GlobalSearchLayout {
            scrim: viewport,
            surface,
            search,
            thread_rows,
            empty_results,
            separator,
            action_rows,
        })
    }

    /// Measures the caret painted by the search field so native IME candidates
    /// remain attached to the actual preedit position.
    pub fn ime_cursor_area(
        metrics: &mut dyn Painter,
        input_rect: Rect,
        input: &TextInputState,
        theme: &ZodeTheme,
    ) -> Option<Rect> {
        if input_rect.size.x <= 0.0 || input_rect.size.y <= 0.0 {
            return None;
        }
        let mut caret_state = input.clone();
        caret_state.set_caret(input.caret(), 0);
        let mut probe = InputCaretProbe {
            metrics,
            caret: None,
        };
        paint_search_input(&mut probe, input_rect, &caret_state, true, theme);
        probe.caret
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        layout: &GlobalSearchLayout,
        input: &TextInputState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        painter.fill_rect(layout.scrim, Color::BLACK.with_alpha(0.10));
        paint_elevated_surface(painter, layout.surface, 12.0, theme);
        painter.fill_round_rect(layout.surface, 12.0, theme.tokens.popover);
        painter.stroke_round_rect(layout.surface, 12.0, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_rect(layout.surface);
        paint_search_input(
            painter,
            layout.search,
            input,
            focused == Some(GLOBAL_SEARCH_INPUT_ID),
            theme,
        );
        for row in &layout.thread_rows {
            paint_row(painter, row, focused, hovered, theme);
        }
        if let Some(empty) = layout.empty_results {
            paint_single_line(
                painter,
                "没有匹配的任务",
                empty,
                13.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Center,
            );
        }
        painter.fill_rect(layout.separator, theme.tokens.border);
        for row in &layout.action_rows {
            paint_row(painter, row, focused, hovered, theme);
        }
        painter.restore();
    }
}

fn normalized_active_index(active_index: usize, selectable_count: usize) -> Option<usize> {
    selectable_count
        .checked_sub(1)
        .map(|last| active_index.min(last))
}

fn paint_search_input(
    painter: &mut dyn Painter,
    rect: Rect,
    input: &TextInputState,
    focused: bool,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(rect, 8.0, theme.tokens.background);
    painter.stroke_round_rect(rect, 8.0, theme.tokens.input, 1.0);
    let icon_size = 16.0;
    painter.stroke_svg_path(
        SemanticIcon::Search.path(),
        Point2D::new(
            rect.origin.x + 12.0,
            rect.origin.y + (rect.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.tokens.muted_foreground,
        SemanticIcon::Search.stroke_width(),
    );
    TextInputView {
        state: input,
        placeholder: "搜索任务或运行命令",
        focused,
        font_size: 14.0,
        now_ms: 0,
        pad_x: 38.0,
        baseline_delta_y: 0.0,
        mask: None,
    }
    .paint(painter, rect, &theme.tokens);
}

fn paint_row(
    painter: &mut dyn Painter,
    row: &GlobalSearchRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let active = row.active || focused == Some(row.id) || hovered == Some(row.id);
    MenuRow::paint_background(painter, row.rect, active, &theme.tokens);
    let icon = match row.target {
        GlobalSearchTarget::Thread(_) => SemanticIcon::Chat,
        GlobalSearchTarget::NewTask => SemanticIcon::NewTask,
        GlobalSearchTarget::OpenFolder => SemanticIcon::Folder,
        GlobalSearchTarget::Settings => SemanticIcon::Settings,
    };
    let icon_origin = Point2D::new(
        row.rect.origin.x + ROW_PAD_X,
        row.rect.origin.y + (row.rect.size.y - ICON_SIZE) / 2.0,
    );
    painter.stroke_svg_path(
        icon.path(),
        icon_origin,
        ICON_SIZE,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
    let text_x = icon_origin.x + ICON_SIZE + ICON_GAP;
    let text_rect = Rect::xywh(
        text_x,
        row.rect.origin.y,
        (row.rect.max_x() - ROW_PAD_X - text_x).max(0.0),
        row.rect.size.y,
    );
    if let Some(detail) = row.detail.as_deref() {
        paint_single_line(
            painter,
            &row.label,
            Rect::xywh(
                text_rect.origin.x,
                text_rect.origin.y + 4.0,
                text_rect.size.x,
                21.0,
            ),
            13.0,
            500,
            theme.tokens.popover_foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            detail,
            Rect::xywh(
                text_rect.origin.x,
                text_rect.origin.y + 23.0,
                text_rect.size.x,
                18.0,
            ),
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    } else {
        paint_single_line(
            painter,
            &row.label,
            text_rect,
            13.0,
            400,
            theme.tokens.popover_foreground,
            HorizontalAlign::Start,
        );
    }
}

struct InputCaretProbe<'a> {
    metrics: &'a mut dyn Painter,
    caret: Option<Rect>,
}

impl Painter for InputCaretProbe<'_> {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, _color: Color) {
        self.caret = Some(rect);
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        self.metrics.dpi_scale()
    }
    fn measure_text_family(&mut self, text: &str, font_size: f32, family: &str) -> f32 {
        self.metrics.measure_text_family(text, font_size, family)
    }
    fn measure_text_family_styled(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.metrics
            .measure_text_family_styled(text, font_size, family, weight, italic)
    }
    fn measure_text_metrics_family_styled(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> TextMetrics {
        self.metrics
            .measure_text_metrics_family_styled(text, font_size, family, weight, italic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_commits_chinese_composition_once() {
        let mut controller = GlobalSearchController::default();
        controller.ime(ImeEvent::Start);
        controller.ime(ImeEvent::Update {
            text: "任务".into(),
            cursor: Some("任务".len()),
        });
        assert_eq!(controller.text(), "");
        controller.ime(ImeEvent::Commit("任务".into()));
        controller.ime(ImeEvent::End);
        assert_eq!(controller.text(), "任务");
    }
}
