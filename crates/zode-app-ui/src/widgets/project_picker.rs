use jian_core::text_input::{prev_char_boundary, TextInputState};
use jian_widgets::{
    components::{input::Input, popover::Popover},
    HorizontalAlign, Painter, Point2D, Rect,
};
use zode_app_model::ZodeAppState;
use zode_node_protocol::WorkspaceUri;

use super::project_sidebar::workspace_label;
use crate::{
    paint_single_line, stable_widget_id, ImeEvent, Key, Modifiers, RectExt, SemanticIcon, WidgetId,
    ZodeTheme,
};

pub const PROJECT_PICKER_TRIGGER_ID: WidgetId = WidgetId(120);
pub const PROJECT_PICKER_SURFACE_ID: WidgetId = WidgetId(121);
pub const PROJECT_PICKER_SEARCH_ID: WidgetId = WidgetId(122);
pub const PROJECT_PICKER_NEW_ID: WidgetId = WidgetId(123);
pub const PROJECT_PICKER_PROJECTLESS_ID: WidgetId = WidgetId(124);

const PROJECT_ROW_NAMESPACE: u8 = 0x74;
const POPOVER_W: f32 = 260.0;
const POPOVER_EDGE_INSET: f32 = 8.0;
const POPOVER_GAP: f32 = 6.0;
const POPOVER_PAD: f32 = 6.0;
const SEARCH_SECTION_H: f32 = 36.0;
const SEARCH_INPUT_H: f32 = 28.0;
const PROJECT_ROW_H: f32 = 29.0;
const MAX_VISIBLE_PROJECTS: usize = 5;
const SEPARATOR_H: f32 = 1.0;
const ACTION_ROW_H: f32 = 29.0;
const ROW_PAD_X: f32 = 12.0;
const ICON_SIZE: f32 = 14.0;
const ICON_GAP: f32 = 8.0;
const TITLE_WIDE_SIZE: f32 = 27.0;
const TITLE_COMPACT_SIZE: f32 = 20.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectPickerViewState {
    pub open: bool,
    pub query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSearchOutcome {
    Ignored,
    Edited,
}

/// Owns the editable search buffer while the immutable UI model carries only
/// the projected query string. Hosts synchronize `text()` after `Edited`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPickerController {
    input: TextInputState,
    now_ms: u64,
}

impl ProjectPickerController {
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

    pub fn key(&mut self, key: Key, modifiers: Modifiers) -> ProjectSearchOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        match key {
            Key::Enter if self.input.composition().is_some() => {
                self.input.commit_composition(self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::Backspace => {
                self.input.backspace(self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::Delete => {
                self.input.delete_forward(self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::ArrowLeft => {
                self.input
                    .move_left(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::ArrowRight => {
                self.input
                    .move_right(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::Home => {
                self.input
                    .move_home(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::End => {
                self.input
                    .move_end(modifiers.contains(Modifiers::SHIFT), self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::Character(character)
                if modifiers.primary() && character.eq_ignore_ascii_case("a") =>
            {
                self.input.select_all();
                ProjectSearchOutcome::Edited
            }
            Key::Character(character)
                if !modifiers.primary() && !modifiers.contains(Modifiers::ALT) =>
            {
                self.input.insert_str(&character, self.now_ms);
                ProjectSearchOutcome::Edited
            }
            Key::Enter
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Tab
            | Key::Escape
            | Key::Character(_) => ProjectSearchOutcome::Ignored,
        }
    }

    pub fn ime(&mut self, event: ImeEvent) -> ProjectSearchOutcome {
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
        ProjectSearchOutcome::Edited
    }

    pub fn paste_text(&mut self, text: &str) -> ProjectSearchOutcome {
        self.now_ms = self.now_ms.saturating_add(1);
        self.input.insert_str(text, self.now_ms);
        ProjectSearchOutcome::Edited
    }
}

impl Default for ProjectPickerController {
    fn default() -> Self {
        Self::new("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WelcomeTitleLayout {
    pub bounds: Rect,
    pub prefix: Rect,
    pub project: Option<Rect>,
    pub suffix: Rect,
    pub font_size: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChoice {
    pub workspace_uri: WorkspaceUri,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectPickerTarget {
    Project(WorkspaceUri),
    NewProject,
    Projectless,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPickerRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub target: ProjectPickerTarget,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPickerLayout {
    pub trigger: Rect,
    pub surface: Rect,
    pub search: Rect,
    pub project_viewport: Rect,
    pub rows: Vec<ProjectPickerRowLayout>,
    pub empty_results: Option<Rect>,
    pub separator: Rect,
    pub new_project: ProjectPickerRowLayout,
    pub projectless: ProjectPickerRowLayout,
}

pub struct ProjectPicker;

impl ProjectPicker {
    pub fn project_widget_id(workspace_uri: &WorkspaceUri) -> WidgetId {
        stable_widget_id(PROJECT_ROW_NAMESPACE, workspace_uri)
    }

    pub fn title_font_size(compact: bool) -> f32 {
        if compact {
            TITLE_COMPACT_SIZE
        } else {
            TITLE_WIDE_SIZE
        }
    }

    pub fn active_workspace_label(state: &ZodeAppState) -> Option<String> {
        let workspace = state.active_available_workspace()?;
        Some(workspace_label(workspace, true))
    }

    pub fn welcome_title_layout(
        bounds: Rect,
        workspace_label: Option<&str>,
        font_size: f32,
    ) -> WelcomeTitleLayout {
        if let Some(project) = workspace_label.filter(|label| !label.trim().is_empty()) {
            let prefix_text = "我们应该在 ";
            let suffix_text = " 中构建什么？";
            let prefix_w = estimated_text_width(prefix_text, font_size);
            let suffix_w = estimated_text_width(suffix_text, font_size);
            let available_project = (bounds.size.x - prefix_w - suffix_w).max(0.0);
            let project_w = estimated_text_width(project, font_size).min(available_project);
            let total = prefix_w + project_w + suffix_w;
            let x = bounds.origin.x + (bounds.size.x - total).max(0.0) / 2.0;
            let prefix = Rect::xywh(x, bounds.origin.y, prefix_w, bounds.size.y);
            let project_rect =
                Rect::xywh(prefix.max_x(), bounds.origin.y, project_w, bounds.size.y);
            let suffix = Rect::xywh(
                project_rect.max_x(),
                bounds.origin.y,
                suffix_w,
                bounds.size.y,
            );
            WelcomeTitleLayout {
                bounds,
                prefix,
                project: (project_w > 0.0).then_some(project_rect),
                suffix,
                font_size,
            }
        } else {
            let text_w = estimated_text_width("我们应该构建什么？", font_size).min(bounds.size.x);
            let text = Rect::xywh(
                bounds.origin.x + (bounds.size.x - text_w).max(0.0) / 2.0,
                bounds.origin.y,
                text_w,
                bounds.size.y,
            );
            WelcomeTitleLayout {
                bounds,
                prefix: text,
                project: None,
                suffix: Rect::xywh(text.max_x(), text.origin.y, 0.0, text.size.y),
                font_size,
            }
        }
    }

    pub fn choices(state: &ZodeAppState, query: &str) -> Vec<ProjectChoice> {
        let normalized_query = query.trim().to_lowercase();
        let mut choices = state
            .projects
            .iter()
            .filter(|project| {
                project.available && !state.is_projectless_workspace(&project.workspace_uri)
            })
            .map(|project| {
                let newest_thread = state
                    .threads
                    .iter()
                    .filter(|thread| thread.workspace_uri == project.workspace_uri)
                    .map(|thread| thread.updated_at_ms)
                    .max()
                    .unwrap_or(i64::MIN);
                let label = workspace_label(&project.workspace_uri, true);
                (
                    ProjectChoice {
                        selected: state.active_workspace.as_ref() == Some(&project.workspace_uri),
                        workspace_uri: project.workspace_uri.clone(),
                        label,
                    },
                    project.last_opened_ms.max(newest_thread),
                )
            })
            .filter(|(choice, _)| {
                normalized_query.is_empty()
                    || choice.label.to_lowercase().contains(&normalized_query)
                    || choice
                        .workspace_uri
                        .as_str()
                        .to_lowercase()
                        .contains(&normalized_query)
            })
            .collect::<Vec<_>>();
        choices.sort_by(|(left, left_key), (right, right_key)| {
            right_key
                .cmp(left_key)
                .then_with(|| left.workspace_uri.cmp(&right.workspace_uri))
        });
        choices.into_iter().map(|(choice, _)| choice).collect()
    }

    pub fn layout(
        viewport: Rect,
        trigger: Rect,
        state: &ZodeAppState,
        picker: &ProjectPickerViewState,
    ) -> Option<ProjectPickerLayout> {
        if !picker.open || trigger.size.x <= 0.0 || trigger.size.y <= 0.0 {
            return None;
        }
        let choices = Self::choices(state, &picker.query);
        let visible = choices.len().min(MAX_VISIBLE_PROJECTS);
        let project_rows = visible.max(1);
        let height = POPOVER_PAD * 2.0
            + SEARCH_SECTION_H
            + project_rows as f32 * PROJECT_ROW_H
            + SEPARATOR_H
            + ACTION_ROW_H * 2.0;
        let surface = centered_placement(trigger, Point2D::new(POPOVER_W, height), viewport);
        if surface.size.x < 120.0 || surface.size.y + f32::EPSILON < height {
            return None;
        }
        let search = Rect::xywh(
            surface.origin.x + POPOVER_PAD,
            surface.origin.y + POPOVER_PAD + (SEARCH_SECTION_H - SEARCH_INPUT_H) / 2.0,
            (surface.size.x - POPOVER_PAD * 2.0).max(0.0),
            SEARCH_INPUT_H,
        );
        let project_viewport = Rect::xywh(
            surface.origin.x + POPOVER_PAD,
            surface.origin.y + POPOVER_PAD + SEARCH_SECTION_H,
            (surface.size.x - POPOVER_PAD * 2.0).max(0.0),
            project_rows as f32 * PROJECT_ROW_H,
        );
        let rows = choices
            .into_iter()
            .take(MAX_VISIBLE_PROJECTS)
            .enumerate()
            .map(|(index, choice)| ProjectPickerRowLayout {
                id: Self::project_widget_id(&choice.workspace_uri),
                rect: Rect::xywh(
                    project_viewport.origin.x,
                    project_viewport.origin.y + index as f32 * PROJECT_ROW_H,
                    project_viewport.size.x,
                    PROJECT_ROW_H,
                ),
                label: choice.label,
                target: ProjectPickerTarget::Project(choice.workspace_uri),
                selected: choice.selected,
            })
            .collect::<Vec<_>>();
        let empty_results = rows.is_empty().then_some(project_viewport);
        let separator = Rect::xywh(
            surface.origin.x + POPOVER_PAD,
            project_viewport.max_y(),
            (surface.size.x - POPOVER_PAD * 2.0).max(0.0),
            SEPARATOR_H,
        );
        let action_x = surface.origin.x + POPOVER_PAD;
        let action_w = (surface.size.x - POPOVER_PAD * 2.0).max(0.0);
        let new_project = ProjectPickerRowLayout {
            id: PROJECT_PICKER_NEW_ID,
            rect: Rect::xywh(action_x, separator.max_y(), action_w, ACTION_ROW_H),
            label: "新建项目".into(),
            target: ProjectPickerTarget::NewProject,
            selected: false,
        };
        let projectless = ProjectPickerRowLayout {
            id: PROJECT_PICKER_PROJECTLESS_ID,
            rect: Rect::xywh(action_x, new_project.rect.max_y(), action_w, ACTION_ROW_H),
            label: "不在项目中工作".into(),
            target: ProjectPickerTarget::Projectless,
            selected: state.active_workspace.is_none(),
        };
        Some(ProjectPickerLayout {
            trigger,
            surface,
            search,
            project_viewport,
            rows,
            empty_results,
            separator,
            new_project,
            projectless,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        layout: &ProjectPickerLayout,
        search_input: &TextInputState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        painter.fill_drop_shadow(
            Rect::xywh(
                layout.surface.origin.x,
                layout.surface.origin.y + 2.0,
                layout.surface.size.x,
                layout.surface.size.y,
            ),
            theme.tokens.radius,
            18.0,
            theme.tokens.foreground.with_alpha(0.10),
        );
        Popover.paint(painter, layout.surface, &theme.tokens);
        painter.save();
        painter.clip_rect(layout.surface);
        Input {
            state: search_input,
            placeholder: "搜索项目",
            focused: focused == Some(PROJECT_PICKER_SEARCH_ID),
            font_size: 13.0,
            now_ms: 0,
            icon_d: Some(SemanticIcon::Search.path()),
        }
        .paint(painter, layout.search, &theme.tokens);
        for row in &layout.rows {
            paint_row(painter, row, focused, hovered, theme);
        }
        if let Some(empty) = layout.empty_results {
            paint_single_line(
                painter,
                "未找到项目",
                empty,
                13.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Center,
            );
        }
        painter.fill_rect(layout.separator, theme.tokens.border);
        paint_row(painter, &layout.new_project, focused, hovered, theme);
        paint_row(painter, &layout.projectless, focused, hovered, theme);
        painter.restore();
    }
}

fn centered_placement(anchor: Rect, requested: Point2D, viewport: Rect) -> Rect {
    let width = requested
        .x
        .min((viewport.size.x - POPOVER_EDGE_INSET * 2.0).max(0.0));
    let height = requested
        .y
        .min((viewport.size.y - POPOVER_EDGE_INSET * 2.0).max(0.0));
    let min_x = viewport.origin.x + POPOVER_EDGE_INSET;
    let max_x = (viewport.max_x() - POPOVER_EDGE_INSET - width).max(min_x);
    let x = (anchor.origin.x + anchor.size.x / 2.0 - width / 2.0).clamp(min_x, max_x);
    let above = anchor.origin.y - POPOVER_GAP - height;
    let below = anchor.max_y() + POPOVER_GAP;
    let min_y = viewport.origin.y + POPOVER_EDGE_INSET;
    let max_y = (viewport.max_y() - POPOVER_EDGE_INSET - height).max(min_y);
    let y = if above >= min_y {
        above
    } else {
        below.clamp(min_y, max_y)
    };
    Rect::xywh(x, y, width, height)
}

fn paint_row(
    painter: &mut dyn Painter,
    row: &ProjectPickerRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if focused == Some(row.id) || hovered == Some(row.id) {
        painter.fill_round_rect(row.rect, theme.tokens.radius, theme.tokens.accent);
        if focused == Some(row.id) {
            painter.stroke_round_rect(row.rect, theme.tokens.radius, theme.tokens.ring, 1.5);
        }
    }
    let icon = match row.target {
        ProjectPickerTarget::Project(_) => SemanticIcon::Folder,
        ProjectPickerTarget::NewProject => SemanticIcon::NewTask,
        ProjectPickerTarget::Projectless => SemanticIcon::Close,
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
    let trailing_reserved = if matches!(row.target, ProjectPickerTarget::NewProject) || row.selected
    {
        ICON_SIZE + ICON_GAP
    } else {
        0.0
    };
    paint_single_line(
        painter,
        &row.label,
        Rect::xywh(
            icon_origin.x + ICON_SIZE + ICON_GAP,
            row.rect.origin.y,
            (row.rect.max_x()
                - ROW_PAD_X
                - trailing_reserved
                - icon_origin.x
                - ICON_SIZE
                - ICON_GAP)
                .max(0.0),
            row.rect.size.y,
        ),
        13.0,
        400,
        theme.tokens.popover_foreground,
        HorizontalAlign::Start,
    );
    let trailing = if matches!(row.target, ProjectPickerTarget::NewProject) {
        Some(SemanticIcon::ChevronRight)
    } else if row.selected {
        Some(SemanticIcon::Check)
    } else {
        None
    };
    if let Some(icon) = trailing {
        painter.stroke_svg_path(
            icon.path(),
            Point2D::new(
                row.rect.max_x() - ROW_PAD_X - ICON_SIZE,
                row.rect.origin.y + (row.rect.size.y - ICON_SIZE) / 2.0,
            ),
            ICON_SIZE,
            theme.tokens.muted_foreground,
            icon.stroke_width(),
        );
    }
}

fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| {
            if character.is_ascii_whitespace() {
                font_size * 0.35
            } else if character.is_ascii() {
                font_size * 0.56
            } else {
                font_size
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_app_model::{demo_state, ProjectState};

    fn workspace(value: &str) -> WorkspaceUri {
        WorkspaceUri::new(value).unwrap()
    }

    #[test]
    fn title_layout_exposes_only_the_dynamic_project_segment() {
        let layout = ProjectPicker::welcome_title_layout(
            Rect::xywh(100.0, 200.0, 700.0, 33.0),
            Some("zode"),
            27.0,
        );
        let project = layout.project.expect("project trigger");
        assert!(project.min_x() >= layout.prefix.max_x() - 0.01);
        assert!(layout.suffix.min_x() >= project.max_x() - 0.01);
        assert!(layout.bounds.contains(Point2D::new(
            project.origin.x + project.size.x / 2.0,
            project.origin.y + project.size.y / 2.0,
        )));
    }

    #[test]
    fn choices_filter_unavailable_projects_and_sort_by_recency() {
        let mut state = demo_state();
        state.projects = vec![
            ProjectState {
                workspace_uri: workspace("file:///repo/older"),
                expanded: true,
                available: true,
                last_opened_ms: 1,
            },
            ProjectState {
                workspace_uri: workspace("file:///repo/newer"),
                expanded: true,
                available: true,
                last_opened_ms: 9,
            },
            ProjectState {
                workspace_uri: workspace("file:///repo/missing"),
                expanded: true,
                available: false,
                last_opened_ms: 99,
            },
        ];
        let labels = ProjectPicker::choices(&state, "er")
            .into_iter()
            .map(|choice| choice.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["newer", "older"]);
    }

    #[test]
    fn popover_is_centered_above_the_trigger_and_caps_project_rows() {
        let mut state = demo_state();
        state.projects = (0..8)
            .map(|index| ProjectState {
                workspace_uri: workspace(&format!("file:///repo/project-{index}")),
                expanded: true,
                available: true,
                last_opened_ms: index,
            })
            .collect();
        let trigger = Rect::xywh(390.0, 330.0, 54.0, 33.0);
        let layout = ProjectPicker::layout(
            Rect::xywh(0.0, 0.0, 864.0, 566.0),
            trigger,
            &state,
            &ProjectPickerViewState {
                open: true,
                query: String::new(),
            },
        )
        .unwrap();
        assert_eq!(layout.rows.len(), 5);
        assert_eq!(layout.surface.size.x, 260.0);
        assert_eq!(layout.surface.size.y, 252.0);
        assert!((layout.surface.origin.x + 130.0 - (trigger.origin.x + 27.0)).abs() < 0.1);
        assert_eq!(layout.surface.max_y(), trigger.origin.y - POPOVER_GAP);
    }

    #[test]
    fn controller_preserves_composition_and_commits_chinese_once() {
        let mut controller = ProjectPickerController::default();
        assert_eq!(
            controller.ime(ImeEvent::Start),
            ProjectSearchOutcome::Edited
        );
        assert_eq!(
            controller.ime(ImeEvent::Update {
                text: "项目".into(),
                cursor: Some("项目".len()),
            }),
            ProjectSearchOutcome::Edited
        );
        assert_eq!(controller.text(), "");
        assert_eq!(
            controller.ime(ImeEvent::Commit("项目".into())),
            ProjectSearchOutcome::Edited
        );
        assert_eq!(controller.ime(ImeEvent::End), ProjectSearchOutcome::Edited);
        assert_eq!(controller.text(), "项目");
    }

    #[test]
    fn picker_omits_an_overlay_that_cannot_contain_its_controls() {
        let mut state = demo_state();
        state.projects.push(ProjectState {
            workspace_uri: workspace("file:///repo/zode"),
            expanded: true,
            available: true,
            last_opened_ms: 1,
        });
        assert!(ProjectPicker::layout(
            Rect::xywh(0.0, 0.0, 180.0, 100.0),
            Rect::xywh(60.0, 40.0, 40.0, 20.0),
            &state,
            &ProjectPickerViewState {
                open: true,
                query: String::new(),
            },
        )
        .is_none());
    }
}
