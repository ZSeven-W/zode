use jian_core::text_input::TextInputState;
use jian_widgets::{
    components::{input::Input, popover::Popover},
    Color, HorizontalAlign, Painter, Point2D, Rect, TextLayout, TextMetrics,
};
use zode_app_model::{
    AppCommand, BranchCatalogState, ComposerContextMenu as ContextMenuKind, TaskLaunchMode,
    ZodeAppState,
};
use zode_node_protocol::WorkspaceUri;

use super::ComposerContextLayout;
use crate::{
    paint_elevated_surface, paint_single_line, stable_widget_id, MenuRow, RectExt, SemanticIcon,
    WidgetId, ZodeTheme,
};

pub const COMPOSER_CONTEXT_MENU_SURFACE_ID: WidgetId = WidgetId(8_500);
pub const COMPOSER_LOCATION_LOCAL_ID: WidgetId = WidgetId(8_501);
pub const COMPOSER_LOCATION_WORKTREE_ID: WidgetId = WidgetId(8_502);
pub const COMPOSER_BRANCH_SEARCH_ID: WidgetId = WidgetId(8_503);
pub const COMPOSER_BRANCH_CREATE_ID: WidgetId = WidgetId(8_504);

const BRANCH_ROW_NAMESPACE: u8 = 0x7b;
const LOCATION_MENU_WIDTH: f32 = 260.0;
const BRANCH_MENU_WIDTH: f32 = 300.0;
const EDGE_INSET: f32 = 8.0;
const ANCHOR_GAP: f32 = 6.0;
const MENU_PADDING: f32 = 6.0;
const TITLE_HEIGHT: f32 = 28.0;
const SEARCH_SECTION_HEIGHT: f32 = 36.0;
const SEARCH_HEIGHT: f32 = 28.0;
const LOCATION_ROW_HEIGHT: f32 = 48.0;
const BRANCH_ROW_HEIGHT: f32 = 42.0;
const CREATE_ROW_HEIGHT: f32 = 34.0;
const SEPARATOR_HEIGHT: f32 = 1.0;
const MAX_VISIBLE_BRANCHES: usize = 7;
const ROW_PAD_X: f32 = 10.0;
const ICON_SIZE: f32 = 15.0;
const ICON_GAP: f32 = 9.0;
const CHECK_SIZE: f32 = 14.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerContextMenuRowTarget {
    LaunchMode(TaskLaunchMode),
    Branch {
        workspace_uri: WorkspaceUri,
        branch: String,
    },
    CreateBranch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerContextMenuRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub secondary: Option<String>,
    pub icon: SemanticIcon,
    pub enabled: bool,
    pub selected: bool,
    pub current: bool,
    pub target: ComposerContextMenuRowTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerContextMenuStatusLayout {
    pub rect: Rect,
    pub message: String,
    pub failed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerContextMenuLayout {
    pub kind: ContextMenuKind,
    pub anchor: Rect,
    pub surface: Rect,
    pub title: Rect,
    pub search: Option<Rect>,
    pub rows: Vec<ComposerContextMenuRowLayout>,
    pub status: Option<ComposerContextMenuStatusLayout>,
    pub separator: Option<Rect>,
    pub create: Option<ComposerContextMenuRowLayout>,
}

pub struct ComposerContextMenu;

impl ComposerContextMenu {
    pub fn branch_widget_id(workspace_uri: &WorkspaceUri, branch: &str) -> WidgetId {
        stable_widget_id(BRANCH_ROW_NAMESPACE, &(workspace_uri, branch))
    }

    pub fn layout(
        viewport: Rect,
        context: ComposerContextLayout,
        state: &ZodeAppState,
    ) -> Option<ComposerContextMenuLayout> {
        if state.current_session.is_some() {
            return None;
        }
        match state.composer.context_menu? {
            ContextMenuKind::Location => {
                let anchor = context.location?.rect;
                Self::location_layout(viewport, anchor, state.composer.launch_mode)
            }
            ContextMenuKind::Branch => {
                let anchor = context.branch?.rect;
                Self::branch_layout(viewport, anchor, state)
            }
        }
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if state.current_session.is_some() {
            return None;
        }
        match state.composer.context_menu? {
            ContextMenuKind::Location => (id == COMPOSER_LOCATION_LOCAL_ID)
                .then_some(AppCommand::SelectTaskLaunchMode(TaskLaunchMode::Local)),
            ContextMenuKind::Branch => {
                if matches!(
                    id,
                    COMPOSER_CONTEXT_MENU_SURFACE_ID
                        | COMPOSER_BRANCH_SEARCH_ID
                        | COMPOSER_BRANCH_CREATE_ID
                ) {
                    return None;
                }
                let workspace_uri = state.active_available_workspace()?.clone();
                let BranchCatalogState::Ready(catalog) = &state.composer.branch_picker.catalog
                else {
                    return None;
                };
                if catalog.workspace_uri != workspace_uri {
                    return None;
                }
                let switching_blocked =
                    catalog.dirty_files > 0 || workspace_has_active_turn(state, &workspace_uri);
                filtered_branches(&catalog.branches, &state.composer.branch_picker.query)
                    .filter(|branch| !switching_blocked || *branch == catalog.current)
                    .find(|branch| Self::branch_widget_id(&workspace_uri, branch) == id)
                    .map(|branch| AppCommand::SelectBranch {
                        workspace_uri,
                        branch: branch.to_owned(),
                    })
            }
        }
    }

    pub fn focus_ids(layout: &ComposerContextMenuLayout) -> Vec<WidgetId> {
        let mut ids = Vec::with_capacity(layout.rows.len() + usize::from(layout.search.is_some()));
        if layout.search.is_some() {
            ids.push(COMPOSER_BRANCH_SEARCH_ID);
        }
        ids.extend(
            layout
                .rows
                .iter()
                .filter(|row| row.enabled)
                .map(|row| row.id),
        );
        ids
    }

    pub fn contains_point(layout: &ComposerContextMenuLayout, point: Point2D) -> bool {
        layout.surface.contains(point)
    }

    pub fn widget_at(layout: &ComposerContextMenuLayout, point: Point2D) -> Option<WidgetId> {
        if !layout.surface.contains(point) {
            return None;
        }
        if layout.search.is_some_and(|rect| rect.contains(point)) {
            return Some(COMPOSER_BRANCH_SEARCH_ID);
        }
        layout
            .rows
            .iter()
            .find(|row| row.enabled && row.rect.contains(point))
            .map(|row| row.id)
            .or(Some(COMPOSER_CONTEXT_MENU_SURFACE_ID))
    }

    /// Measures the same caret painted by the branch search input so the
    /// native IME candidate window follows text instead of the menu corner.
    pub fn branch_search_ime_cursor_area(
        metrics: &mut dyn Painter,
        search: Rect,
        input: &TextInputState,
        theme: &ZodeTheme,
    ) -> Option<Rect> {
        if search.size.x <= 0.0 || search.size.y <= 0.0 {
            return None;
        }
        let mut caret_state = input.clone();
        caret_state.set_caret(input.caret(), 0);
        let mut probe = InputCaretProbe {
            metrics,
            caret: None,
        };
        Input {
            state: &caret_state,
            placeholder: "搜索分支",
            focused: true,
            font_size: 13.0,
            now_ms: 0,
            icon_d: Some(SemanticIcon::Search.path()),
        }
        .paint(&mut probe, search, &theme.tokens);
        probe.caret
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        layout: &ComposerContextMenuLayout,
        branch_search_input: &TextInputState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        // `Popover::paint` fills/strokes at its own fixed 6.0 radius
        // (`jian_widgets::components::popover`, which equals `tokens.radius`
        // here) - the shadow now matches that instead of the previous ad hoc
        // 11.0.
        paint_elevated_surface(painter, layout.surface, theme.tokens.radius, theme);
        Popover.paint(painter, layout.surface, &theme.tokens);
        painter.save();
        painter.clip_rect(layout.surface);
        paint_single_line(
            painter,
            match layout.kind {
                ContextMenuKind::Location => "启动模式",
                ContextMenuKind::Branch => "分支",
            },
            layout.title,
            12.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        if let Some(search) = layout.search {
            Input {
                state: branch_search_input,
                placeholder: "搜索分支",
                focused: focused == Some(COMPOSER_BRANCH_SEARCH_ID),
                font_size: 13.0,
                now_ms: 0,
                icon_d: Some(SemanticIcon::Search.path()),
            }
            .paint(painter, search, &theme.tokens);
        }
        for row in &layout.rows {
            paint_row(painter, row, focused, hovered, theme);
        }
        if let Some(status) = &layout.status {
            paint_single_line(
                painter,
                &status.message,
                status.rect,
                12.0,
                400,
                if status.failed {
                    theme.tokens.destructive
                } else {
                    theme.tokens.muted_foreground
                },
                HorizontalAlign::Center,
            );
        }
        if let Some(separator) = layout.separator {
            painter.fill_rect(separator, theme.tokens.border);
        }
        if let Some(create) = &layout.create {
            paint_row(painter, create, focused, hovered, theme);
        }
        painter.restore();
    }

    fn location_layout(
        viewport: Rect,
        anchor: Rect,
        selected: TaskLaunchMode,
    ) -> Option<ComposerContextMenuLayout> {
        let desired_height = MENU_PADDING * 2.0 + TITLE_HEIGHT + LOCATION_ROW_HEIGHT * 2.0;
        let surface = place_above(viewport, anchor, LOCATION_MENU_WIDTH, desired_height)?;
        let content_x = surface.origin.x + MENU_PADDING;
        let content_width = (surface.size.x - MENU_PADDING * 2.0).max(0.0);
        let title = Rect::xywh(
            content_x + ROW_PAD_X,
            surface.origin.y + MENU_PADDING,
            (content_width - ROW_PAD_X * 2.0).max(0.0),
            TITLE_HEIGHT,
        );
        let first_y = title.max_y();
        let rows = vec![
            ComposerContextMenuRowLayout {
                id: COMPOSER_LOCATION_LOCAL_ID,
                rect: Rect::xywh(content_x, first_y, content_width, LOCATION_ROW_HEIGHT),
                label: "在本地处理".into(),
                secondary: None,
                icon: SemanticIcon::Host,
                enabled: true,
                selected: selected == TaskLaunchMode::Local,
                current: selected == TaskLaunchMode::Local,
                target: ComposerContextMenuRowTarget::LaunchMode(TaskLaunchMode::Local),
            },
            ComposerContextMenuRowLayout {
                id: COMPOSER_LOCATION_WORKTREE_ID,
                rect: Rect::xywh(
                    content_x,
                    first_y + LOCATION_ROW_HEIGHT,
                    content_width,
                    LOCATION_ROW_HEIGHT,
                ),
                label: "新工作树".into(),
                secondary: Some("尚未接入工作树创建".into()),
                icon: SemanticIcon::Worktree,
                enabled: false,
                selected: selected == TaskLaunchMode::Worktree,
                current: false,
                target: ComposerContextMenuRowTarget::LaunchMode(TaskLaunchMode::Worktree),
            },
        ];
        Some(ComposerContextMenuLayout {
            kind: ContextMenuKind::Location,
            anchor,
            surface,
            title,
            search: None,
            rows,
            status: None,
            separator: None,
            create: None,
        })
    }

    fn branch_layout(
        viewport: Rect,
        anchor: Rect,
        state: &ZodeAppState,
    ) -> Option<ComposerContextMenuLayout> {
        let workspace = state.active_available_workspace();
        let available_height = (viewport.size.y - EDGE_INSET * 2.0).max(0.0);
        let fixed_height = MENU_PADDING * 2.0
            + SEARCH_SECTION_HEIGHT
            + TITLE_HEIGHT
            + SEPARATOR_HEIGHT
            + CREATE_ROW_HEIGHT;
        let row_capacity = (((available_height - fixed_height) / BRANCH_ROW_HEIGHT).floor()
            as isize)
            .clamp(1, MAX_VISIBLE_BRANCHES as isize) as usize;

        let (row_data, message) = branch_contents(state, workspace, row_capacity);
        let content_rows = row_data.len().max(1);
        let desired_height = fixed_height + content_rows as f32 * BRANCH_ROW_HEIGHT;
        let surface = place_above(viewport, anchor, BRANCH_MENU_WIDTH, desired_height)?;
        let content_x = surface.origin.x + MENU_PADDING;
        let content_width = (surface.size.x - MENU_PADDING * 2.0).max(0.0);
        let search = Rect::xywh(
            content_x,
            surface.origin.y
                + MENU_PADDING
                + (SEARCH_SECTION_HEIGHT - SEARCH_HEIGHT).max(0.0) / 2.0,
            content_width,
            SEARCH_HEIGHT,
        );
        let title = Rect::xywh(
            content_x + ROW_PAD_X,
            surface.origin.y + MENU_PADDING + SEARCH_SECTION_HEIGHT,
            (content_width - ROW_PAD_X * 2.0).max(0.0),
            TITLE_HEIGHT,
        );
        let rows_y = title.max_y();
        let rows = row_data
            .into_iter()
            .enumerate()
            .map(|(index, data)| ComposerContextMenuRowLayout {
                id: Self::branch_widget_id(&data.workspace_uri, &data.branch),
                rect: Rect::xywh(
                    content_x,
                    rows_y + index as f32 * BRANCH_ROW_HEIGHT,
                    content_width,
                    BRANCH_ROW_HEIGHT,
                ),
                label: data.branch.clone(),
                secondary: data.secondary,
                icon: SemanticIcon::Branch,
                enabled: data.enabled,
                selected: data.selected,
                current: data.current,
                target: ComposerContextMenuRowTarget::Branch {
                    workspace_uri: data.workspace_uri,
                    branch: data.branch,
                },
            })
            .collect::<Vec<_>>();
        let status = message.map(|(message, failed)| ComposerContextMenuStatusLayout {
            rect: Rect::xywh(content_x, rows_y, content_width, BRANCH_ROW_HEIGHT),
            message,
            failed,
        });
        let content_bottom = rows_y + content_rows as f32 * BRANCH_ROW_HEIGHT;
        let separator = Rect::xywh(content_x, content_bottom, content_width, SEPARATOR_HEIGHT);
        let create = ComposerContextMenuRowLayout {
            id: COMPOSER_BRANCH_CREATE_ID,
            rect: Rect::xywh(
                content_x,
                separator.max_y(),
                content_width,
                CREATE_ROW_HEIGHT,
            ),
            label: "创建并检出新分支…".into(),
            secondary: Some("尚未接入".into()),
            icon: SemanticIcon::Plus,
            enabled: false,
            selected: false,
            current: false,
            target: ComposerContextMenuRowTarget::CreateBranch,
        };
        Some(ComposerContextMenuLayout {
            kind: ContextMenuKind::Branch,
            anchor,
            surface,
            title,
            search: Some(search),
            rows,
            status,
            separator: Some(separator),
            create: Some(create),
        })
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

struct BranchRowData {
    workspace_uri: WorkspaceUri,
    branch: String,
    secondary: Option<String>,
    enabled: bool,
    selected: bool,
    current: bool,
}

fn branch_contents(
    state: &ZodeAppState,
    workspace: Option<&WorkspaceUri>,
    limit: usize,
) -> (Vec<BranchRowData>, Option<(String, bool)>) {
    let Some(workspace_uri) = workspace else {
        return (Vec::new(), Some(("没有可用项目".into(), true)));
    };
    match &state.composer.branch_picker.catalog {
        BranchCatalogState::Ready(catalog) if &catalog.workspace_uri == workspace_uri => {
            let workspace_has_active_turn = workspace_has_active_turn(state, workspace_uri);
            let switching_blocked = catalog.dirty_files > 0 || workspace_has_active_turn;
            let selected = state
                .composer
                .selected_branch
                .as_deref()
                .unwrap_or(&catalog.current);
            let rows = filtered_branches(&catalog.branches, &state.composer.branch_picker.query)
                .take(limit)
                .map(|branch| {
                    let current = branch == catalog.current;
                    BranchRowData {
                        workspace_uri: workspace_uri.clone(),
                        branch: branch.to_owned(),
                        secondary: if current && catalog.dirty_files > 0 {
                            Some(format!("未提交：{} 个文件", catalog.dirty_files))
                        } else if !current && catalog.dirty_files > 0 {
                            Some("请先提交或储藏更改".into())
                        } else if !current && workspace_has_active_turn {
                            Some("工作区有运行中的任务".into())
                        } else {
                            None
                        },
                        enabled: current || !switching_blocked,
                        selected: branch == selected,
                        current,
                    }
                })
                .collect::<Vec<_>>();
            let message = rows.is_empty().then(|| ("未找到分支".into(), false));
            (rows, message)
        }
        BranchCatalogState::Loading {
            workspace_uri: loading,
        } if loading == workspace_uri => (Vec::new(), Some(("正在读取分支…".into(), false))),
        BranchCatalogState::Switching {
            workspace_uri: switching,
            branch,
            ..
        } if switching == workspace_uri => {
            (Vec::new(), Some((format!("正在切换到 {branch}…"), false)))
        }
        BranchCatalogState::Failed {
            workspace_uri: failed,
            message,
        } if failed == workspace_uri => (Vec::new(), Some((message.clone(), true))),
        _ => (Vec::new(), Some(("正在读取分支…".into(), false))),
    }
}

fn workspace_has_active_turn(state: &ZodeAppState, workspace_uri: &WorkspaceUri) -> bool {
    state.active_turns.keys().any(|session| {
        state
            .threads
            .iter()
            .find(|thread| &thread.session == session)
            .is_some_and(|thread| &thread.workspace_uri == workspace_uri)
    })
}

fn filtered_branches<'a>(branches: &'a [String], query: &str) -> impl Iterator<Item = &'a str> {
    let normalized = query.trim().to_lowercase();
    branches
        .iter()
        .map(String::as_str)
        .filter(move |branch| normalized.is_empty() || branch.to_lowercase().contains(&normalized))
}

fn place_above(
    viewport: Rect,
    anchor: Rect,
    requested_width: f32,
    requested_height: f32,
) -> Option<Rect> {
    let width = requested_width.min((viewport.size.x - EDGE_INSET * 2.0).max(0.0));
    let height = requested_height.min((viewport.size.y - EDGE_INSET * 2.0).max(0.0));
    if width < 120.0 || height + f32::EPSILON < requested_height {
        return None;
    }
    let min_x = viewport.origin.x + EDGE_INSET;
    let max_x = (viewport.max_x() - EDGE_INSET - width).max(min_x);
    let x = (anchor.origin.x + anchor.size.x / 2.0 - width / 2.0).clamp(min_x, max_x);
    let min_y = viewport.origin.y + EDGE_INSET;
    let max_y = (viewport.max_y() - EDGE_INSET - height).max(min_y);
    let y = (anchor.origin.y - ANCHOR_GAP - height).clamp(min_y, max_y);
    Some(Rect::xywh(x, y, width, height))
}

fn paint_row(
    painter: &mut dyn Painter,
    row: &ComposerContextMenuRowLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let active = row.enabled && (focused == Some(row.id) || hovered == Some(row.id));
    MenuRow::paint_background(painter, row.rect, active, &theme.tokens);
    let foreground = if row.enabled {
        theme.tokens.popover_foreground
    } else {
        theme.tokens.muted_foreground.with_alpha(0.62)
    };
    let icon = Rect::xywh(
        row.rect.origin.x + ROW_PAD_X,
        row.rect.origin.y + (row.rect.size.y - ICON_SIZE) / 2.0,
        ICON_SIZE,
        ICON_SIZE,
    );
    painter.stroke_svg_path(
        row.icon.path(),
        icon.origin,
        ICON_SIZE,
        foreground,
        row.icon.stroke_width(),
    );
    let text_x = icon.max_x() + ICON_GAP;
    let trailing = CHECK_SIZE + ROW_PAD_X * 2.0;
    let text_rect = Rect::xywh(
        text_x,
        row.rect.origin.y,
        (row.rect.max_x() - trailing - text_x).max(0.0),
        row.rect.size.y,
    );
    if let Some(secondary) = &row.secondary {
        paint_single_line(
            painter,
            &row.label,
            Rect::xywh(
                text_rect.origin.x,
                text_rect.origin.y + 4.0,
                text_rect.size.x,
                20.0,
            ),
            13.0,
            if row.selected { 500 } else { 400 },
            foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            secondary,
            Rect::xywh(
                text_rect.origin.x,
                text_rect.origin.y + 22.0,
                text_rect.size.x,
                17.0,
            ),
            11.0,
            400,
            theme
                .tokens
                .muted_foreground
                .with_alpha(if row.enabled { 1.0 } else { 0.62 }),
            HorizontalAlign::Start,
        );
    } else {
        paint_single_line(
            painter,
            &row.label,
            text_rect,
            13.0,
            if row.selected { 500 } else { 400 },
            foreground,
            HorizontalAlign::Start,
        );
    }
    if row.selected {
        let check = Rect::xywh(
            row.rect.max_x() - ROW_PAD_X - CHECK_SIZE,
            row.rect.origin.y + (row.rect.size.y - CHECK_SIZE) / 2.0,
            CHECK_SIZE,
            CHECK_SIZE,
        );
        painter.stroke_svg_path(
            SemanticIcon::Check.path(),
            check.origin,
            CHECK_SIZE,
            foreground,
            SemanticIcon::Check.stroke_width(),
        );
    }
}
