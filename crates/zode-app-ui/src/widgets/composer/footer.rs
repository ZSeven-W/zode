use jian_widgets::{components::popover::Popover, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, ComposerFooterMenu, SettingsCategory, ShellRoute, ZodeAppState};
use zode_node_protocol::{ApprovalMode, SandboxMode};

use crate::{
    paint_elevated_surface, paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId,
    ZodeTheme,
};

#[path = "footer-entries.rs"]
mod footer_entries;
use footer_entries::{add_entries, model_entries, permission_entries, MenuEntry};

pub const COMPOSER_ADD_ID: WidgetId = WidgetId(8_600);
pub const COMPOSER_PERMISSION_ID: WidgetId = WidgetId(8_601);
pub const COMPOSER_MODEL_ID: WidgetId = WidgetId(8_602);
pub const COMPOSER_FOOTER_MENU_SURFACE_ID: WidgetId = WidgetId(8_603);
pub const COMPOSER_PERMISSION_REQUEST_ID: WidgetId = WidgetId(8_610);
pub const COMPOSER_PERMISSION_AUTO_ID: WidgetId = WidgetId(8_611);
pub const COMPOSER_PERMISSION_FULL_ID: WidgetId = WidgetId(8_612);
pub const COMPOSER_PERMISSION_CUSTOM_ID: WidgetId = WidgetId(8_613);
pub const COMPOSER_ADD_FILE_ID: WidgetId = WidgetId(8_620);
pub const COMPOSER_ADD_WECHAT_ID: WidgetId = WidgetId(8_621);
pub const COMPOSER_ADD_GOAL_ID: WidgetId = WidgetId(8_622);
pub const COMPOSER_ADD_PLAN_ID: WidgetId = WidgetId(8_623);
pub const COMPOSER_MODEL_EFFORT_LOW_ID: WidgetId = WidgetId(8_630);
pub const COMPOSER_MODEL_EFFORT_MEDIUM_ID: WidgetId = WidgetId(8_631);
pub const COMPOSER_MODEL_EFFORT_HIGH_ID: WidgetId = WidgetId(8_632);
pub const COMPOSER_MODEL_EFFORT_XHIGH_ID: WidgetId = WidgetId(8_633);
pub const COMPOSER_MODEL_SPEED_ID: WidgetId = WidgetId(8_634);
pub const COMPOSER_MODEL_RESET_ID: WidgetId = WidgetId(8_635);
pub const COMPOSER_MODEL_CONFIGURE_ID: WidgetId = WidgetId(8_636);
pub const COMPOSER_MODEL_MODELS_ID: WidgetId = WidgetId(8_637);
pub const COMPOSER_MODEL_EFFORTS_ID: WidgetId = WidgetId(8_638);
pub const COMPOSER_MODEL_SPEEDS_ID: WidgetId = WidgetId(8_639);
pub const COMPOSER_MODEL_BACK_ID: WidgetId = WidgetId(8_640);
pub const COMPOSER_MODEL_ADD_ID: WidgetId = WidgetId(8_641);

const MODEL_NAMESPACE: u8 = 0x7d;
const CONTROL_H: f32 = 28.0;
const FOOTER_LABEL_SIZE: f32 = 14.0;
const MENU_GAP: f32 = 7.0;
const MENU_PAD: f32 = 6.0;
const SECTION_H: f32 = 26.0;
const ROW_H: f32 = 48.0;
const COMPACT_ROW_H: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerFooterLayout {
    pub add: Rect,
    pub permission: Rect,
    pub model: Rect,
    pub send: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerFooterRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: String,
    pub detail: Option<String>,
    pub icon: SemanticIcon,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerFooterSectionLayout {
    pub rect: Rect,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposerFooterMenuLayout {
    pub kind: ComposerFooterMenu,
    pub surface: Rect,
    pub sections: Vec<ComposerFooterSectionLayout>,
    pub rows: Vec<ComposerFooterRowLayout>,
}

pub struct ComposerFooterMenuWidget;

impl ComposerFooterMenuWidget {
    pub fn trigger_layout(input: Rect, state: &ZodeAppState) -> ComposerFooterLayout {
        let y = input.max_y() - 38.0;
        let add = Rect::xywh(input.origin.x + 10.0, y, CONTROL_H, CONTROL_H);
        let permission_w =
            (footer_label_width(&state.composer.sandbox_label) + 38.0).clamp(84.0, 150.0);
        let permission = Rect::xywh(add.max_x() + 4.0, y, permission_w, CONTROL_H);
        let send = Rect::xywh(input.max_x() - 42.0, y, CONTROL_H, CONTROL_H);
        let available = (send.origin.x - permission.max_x() - 12.0).max(0.0);
        let model_w = available.clamp(0.0, 190.0);
        let model = Rect::xywh(send.origin.x - model_w - 6.0, y, model_w, CONTROL_H);
        ComposerFooterLayout {
            add,
            permission,
            model,
            send,
        }
    }

    pub fn model_widget_id(model: &str) -> WidgetId {
        stable_widget_id(MODEL_NAMESPACE, &model)
    }

    pub fn layout(
        viewport: Rect,
        input: Rect,
        state: &ZodeAppState,
    ) -> Option<ComposerFooterMenuLayout> {
        let kind = state.composer.footer_menu?;
        let triggers = Self::trigger_layout(input, state);
        let anchor = match kind {
            ComposerFooterMenu::Add => triggers.add,
            ComposerFooterMenu::Permission => triggers.permission,
            ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed => triggers.model,
        };
        let (width, entries) = match kind {
            ComposerFooterMenu::Add => (input.size.x, add_entries(state)),
            ComposerFooterMenu::Permission => (356.0, permission_entries(state)),
            ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed => (300.0, model_entries(state)),
        };
        let height = entries.iter().map(MenuEntry::height).sum::<f32>() + MENU_PAD * 2.0;
        let desired_x = match kind {
            ComposerFooterMenu::Add => input.origin.x,
            ComposerFooterMenu::Permission => anchor.origin.x,
            ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed => anchor.max_x() - width,
        };
        let surface_width = width.min((viewport.size.x - 16.0).max(0.0));
        let min_x = viewport.origin.x + 8.0;
        let max_x = viewport.max_x() - surface_width - 8.0;
        let x = if min_x <= max_x {
            desired_x.clamp(min_x, max_x)
        } else {
            viewport.origin.x + (viewport.size.x - surface_width) / 2.0
        };
        let y = (anchor.origin.y - MENU_GAP - height).max(viewport.origin.y + 8.0);
        let surface = Rect::xywh(x, y, surface_width, height);
        let mut cursor_y = surface.origin.y + MENU_PAD;
        let mut sections = Vec::new();
        let mut rows = Vec::new();
        for entry in entries {
            match entry {
                MenuEntry::Section(label) => {
                    sections.push(ComposerFooterSectionLayout {
                        rect: Rect::xywh(
                            surface.origin.x + MENU_PAD,
                            cursor_y,
                            surface.size.x - MENU_PAD * 2.0,
                            SECTION_H,
                        ),
                        label,
                    });
                    cursor_y += SECTION_H;
                }
                MenuEntry::Row(mut row, compact) => {
                    let height = if compact { COMPACT_ROW_H } else { ROW_H };
                    row.rect = Rect::xywh(
                        surface.origin.x + MENU_PAD,
                        cursor_y,
                        surface.size.x - MENU_PAD * 2.0,
                        height,
                    );
                    cursor_y += height;
                    rows.push(row);
                }
            }
        }
        Some(ComposerFooterMenuLayout {
            kind,
            surface,
            sections,
            rows,
        })
    }

    pub fn widget_at(layout: &ComposerFooterMenuLayout, point: Point2D) -> Option<WidgetId> {
        if !layout.surface.contains(point) {
            return None;
        }
        layout
            .rows
            .iter()
            .find(|row| row.enabled && row.rect.contains(point))
            .map(|row| row.id)
            .or(Some(COMPOSER_FOOTER_MENU_SURFACE_ID))
    }

    pub fn focus_ids(layout: &ComposerFooterMenuLayout) -> Vec<WidgetId> {
        layout
            .rows
            .iter()
            .filter(|row| row.enabled)
            .map(|row| row.id)
            .collect()
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        let menu = state.composer.footer_menu?;
        if menu == ComposerFooterMenu::Permission {
            if runtime_controls_locked(state) {
                return None;
            }
            return match id {
                COMPOSER_PERMISSION_REQUEST_ID => Some(AppCommand::SetPermissionPreset {
                    approval_mode: ApprovalMode::Request,
                    sandbox_mode: SandboxMode::WorkspaceWrite,
                    network: false,
                }),
                COMPOSER_PERMISSION_AUTO_ID => Some(AppCommand::SetPermissionPreset {
                    approval_mode: ApprovalMode::Auto,
                    sandbox_mode: SandboxMode::WorkspaceWrite,
                    network: true,
                }),
                COMPOSER_PERMISSION_FULL_ID => Some(AppCommand::SetPermissionPreset {
                    approval_mode: ApprovalMode::Full,
                    sandbox_mode: SandboxMode::Off,
                    network: false,
                }),
                COMPOSER_PERMISSION_CUSTOM_ID => Some(AppCommand::Navigate(ShellRoute::Settings(
                    SettingsCategory::Configuration,
                ))),
                _ => None,
            };
        }
        match (menu, id) {
            (ComposerFooterMenu::Model, COMPOSER_MODEL_MODELS_ID) => {
                return Some(AppCommand::ToggleComposerFooterMenu(
                    ComposerFooterMenu::ModelModels,
                ));
            }
            (ComposerFooterMenu::Model, COMPOSER_MODEL_EFFORTS_ID) => {
                return Some(AppCommand::ToggleComposerFooterMenu(
                    ComposerFooterMenu::ModelEffort,
                ));
            }
            (ComposerFooterMenu::Model, COMPOSER_MODEL_SPEEDS_ID) => {
                return Some(AppCommand::ToggleComposerFooterMenu(
                    ComposerFooterMenu::ModelSpeed,
                ));
            }
            (
                ComposerFooterMenu::ModelModels
                | ComposerFooterMenu::ModelEffort
                | ComposerFooterMenu::ModelSpeed,
                COMPOSER_MODEL_BACK_ID,
            ) => {
                return Some(AppCommand::ToggleComposerFooterMenu(
                    ComposerFooterMenu::Model,
                ));
            }
            _ => {}
        }
        if menu == ComposerFooterMenu::ModelModels
            && id == COMPOSER_MODEL_CONFIGURE_ID
            && state.provider_setup_required
        {
            return Some(AppCommand::Navigate(ShellRoute::Settings(
                SettingsCategory::ProviderModels,
            )));
        }
        if menu == ComposerFooterMenu::ModelModels && id == COMPOSER_MODEL_ADD_ID {
            return Some(AppCommand::Navigate(ShellRoute::Settings(
                SettingsCategory::ProviderModels,
            )));
        }
        if runtime_controls_locked(state) {
            return None;
        }
        if menu == ComposerFooterMenu::ModelModels {
            if let Some(model) = state
                .composer
                .available_models
                .iter()
                .find(|model| Self::model_widget_id(model) == id)
            {
                return Some(AppCommand::SetModel(model.clone()));
            }
        }
        match (menu, id) {
            (ComposerFooterMenu::ModelEffort, COMPOSER_MODEL_EFFORT_LOW_ID) => {
                Some(AppCommand::SetEffort("low".into()))
            }
            (ComposerFooterMenu::ModelEffort, COMPOSER_MODEL_EFFORT_MEDIUM_ID) => {
                Some(AppCommand::SetEffort("medium".into()))
            }
            (ComposerFooterMenu::ModelEffort, COMPOSER_MODEL_EFFORT_HIGH_ID) => {
                Some(AppCommand::SetEffort("high".into()))
            }
            (ComposerFooterMenu::ModelEffort, COMPOSER_MODEL_EFFORT_XHIGH_ID) => {
                Some(AppCommand::SetEffort("xhigh".into()))
            }
            (ComposerFooterMenu::Model, COMPOSER_MODEL_RESET_ID)
                if state.composer_defaults.is_some() =>
            {
                Some(AppCommand::ResetComposerRuntime)
            }
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_controls(
        painter: &mut dyn Painter,
        layout: ComposerFooterLayout,
        state: &ZodeAppState,
        has_content: bool,
        busy: bool,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        control_background(
            painter,
            layout.add,
            COMPOSER_ADD_ID,
            state.composer.footer_menu == Some(ComposerFooterMenu::Add),
            focused,
            hovered,
            theme,
        );
        paint_icon(
            painter,
            SemanticIcon::Plus,
            layout.add,
            theme.tokens.foreground,
            theme,
        );

        control_background(
            painter,
            layout.permission,
            COMPOSER_PERMISSION_ID,
            state.composer.footer_menu == Some(ComposerFooterMenu::Permission),
            focused,
            hovered,
            theme,
        );
        let (permission_semantic, permission_color) = permission_visual(state, theme);
        let permission_icon = Rect::xywh(
            layout.permission.origin.x + 8.0,
            layout.permission.origin.y + 6.0,
            16.0,
            16.0,
        );
        painter.stroke_svg_path(
            permission_semantic.path(),
            permission_icon.origin,
            permission_icon.size.x,
            permission_color,
            permission_semantic.stroke_width(),
        );
        paint_single_line(
            painter,
            &state.composer.sandbox_label,
            Rect::xywh(
                permission_icon.max_x() + 5.0,
                layout.permission.origin.y,
                (layout.permission.max_x() - permission_icon.max_x() - 10.0).max(0.0),
                layout.permission.size.y,
            ),
            FOOTER_LABEL_SIZE,
            400,
            permission_color,
            HorizontalAlign::Start,
        );

        if layout.model.size.x > 0.0 {
            control_background(
                painter,
                layout.model,
                COMPOSER_MODEL_ID,
                state.composer.footer_menu.is_some_and(is_model_menu),
                focused,
                hovered,
                theme,
            );
            let model = if state.provider_setup_required {
                "Codex（内置）· 尚未配置".to_owned()
            } else {
                let model = state.composer.model.as_deref().unwrap_or("选择模型");
                match state.composer.effort.as_deref() {
                    Some(effort) => format!("{model} {}", effort_label(effort)),
                    None => model.to_owned(),
                }
            };
            paint_single_line(
                painter,
                &model,
                Rect::xywh(
                    layout.model.origin.x + 10.0,
                    layout.model.origin.y,
                    (layout.model.size.x - 30.0).max(0.0),
                    layout.model.size.y,
                ),
                FOOTER_LABEL_SIZE,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::End,
            );
            let chevron = Rect::xywh(
                layout.model.max_x() - 18.0,
                layout.model.origin.y + 7.0,
                14.0,
                14.0,
            );
            painter.stroke_svg_path(
                SemanticIcon::ChevronDown.path(),
                chevron.origin,
                chevron.size.x,
                theme.tokens.muted_foreground,
                SemanticIcon::ChevronDown.stroke_width(),
            );
        }

        if busy {
            painter.fill_round_rect(layout.send, 14.0, theme.tokens.foreground);
            painter.fill_round_rect(
                Rect::xywh(
                    layout.send.origin.x + 10.0,
                    layout.send.origin.y + 10.0,
                    8.0,
                    8.0,
                ),
                1.5,
                theme.tokens.background,
            );
        } else {
            let color = if has_content {
                theme.zode_purple
            } else {
                theme.tokens.muted_foreground.with_alpha(0.72)
            };
            painter.fill_round_rect(layout.send, 14.0, color);
            paint_icon(
                painter,
                SemanticIcon::Send,
                layout.send,
                jian_widgets::Color::WHITE,
                theme,
            );
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        layout: &ComposerFooterMenuLayout,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        // `Popover::paint` fills/strokes at its own fixed 6.0 radius
        // (`jian_widgets::components::popover`, which equals `tokens.radius`
        // here) - the shadow now matches that instead of the previous ad hoc
        // 12.0.
        paint_elevated_surface(painter, layout.surface, theme.tokens.radius, theme);
        Popover.paint(painter, layout.surface, &theme.tokens);
        painter.save();
        painter.clip_rect(layout.surface);
        for section in &layout.sections {
            paint_single_line(
                painter,
                &section.label,
                Rect::xywh(
                    section.rect.origin.x + 6.0,
                    section.rect.origin.y,
                    section.rect.size.x - 12.0,
                    section.rect.size.y,
                ),
                11.0,
                500,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        for row in &layout.rows {
            if row.enabled && (hovered == Some(row.id) || focused == Some(row.id)) {
                painter.fill_round_rect(row.rect, 8.0, theme.tokens.muted);
            }
            let color = if row.enabled {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground.with_alpha(0.52)
            };
            let icon = Rect::xywh(
                row.rect.origin.x + 10.0,
                row.rect.origin.y + 10.0,
                17.0,
                17.0,
            );
            painter.stroke_svg_path(
                row.icon.path(),
                icon.origin,
                icon.size.x,
                color,
                row.icon.stroke_width(),
            );
            let text_x = icon.max_x() + 10.0;
            paint_single_line(
                painter,
                &row.label,
                Rect::xywh(
                    text_x,
                    row.rect.origin.y + if row.detail.is_some() { 3.0 } else { 0.0 },
                    (row.rect.max_x() - text_x - 36.0).max(0.0),
                    if row.detail.is_some() {
                        24.0
                    } else {
                        row.rect.size.y
                    },
                ),
                13.0,
                500,
                color,
                HorizontalAlign::Start,
            );
            if let Some(detail) = row.detail.as_deref() {
                paint_single_line(
                    painter,
                    detail,
                    Rect::xywh(
                        text_x,
                        row.rect.origin.y + 23.0,
                        row.rect.max_x() - text_x - 12.0,
                        20.0,
                    ),
                    10.5,
                    400,
                    theme.tokens.muted_foreground,
                    HorizontalAlign::Start,
                );
            }
            if row.selected {
                let check = Rect::xywh(
                    row.rect.max_x() - 24.0,
                    row.rect.origin.y + (row.rect.size.y - 14.0) / 2.0,
                    14.0,
                    14.0,
                );
                painter.stroke_svg_path(
                    SemanticIcon::Check.path(),
                    check.origin,
                    check.size.x,
                    theme.tokens.foreground,
                    SemanticIcon::Check.stroke_width(),
                );
            }
            if is_model_disclosure(row.id) {
                let chevron = Rect::xywh(
                    row.rect.max_x() - 24.0,
                    row.rect.origin.y + (row.rect.size.y - 14.0) / 2.0,
                    14.0,
                    14.0,
                );
                painter.stroke_svg_path(
                    SemanticIcon::ChevronRight.path(),
                    chevron.origin,
                    chevron.size.x,
                    color,
                    SemanticIcon::ChevronRight.stroke_width(),
                );
            }
        }
        painter.restore();
    }
}

fn permission_visual(
    state: &ZodeAppState,
    theme: &ZodeTheme,
) -> (SemanticIcon, jian_widgets::Color) {
    match (
        state.composer.approval_mode,
        state.composer.sandbox_mode,
        state.composer.sandbox_network,
    ) {
        (ApprovalMode::Request, SandboxMode::WorkspaceWrite, false) => {
            (SemanticIcon::Host, theme.tokens.muted_foreground)
        }
        (ApprovalMode::Auto, SandboxMode::WorkspaceWrite, true) => {
            (SemanticIcon::Refresh, theme.tokens.muted_foreground)
        }
        (ApprovalMode::Full, SandboxMode::Off, _) => {
            (SemanticIcon::ShieldAlert, theme.composer_permission)
        }
        _ => (SemanticIcon::Settings, theme.tokens.muted_foreground),
    }
}

fn footer_label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| {
            if character.is_ascii() {
                FOOTER_LABEL_SIZE * 0.56
            } else {
                FOOTER_LABEL_SIZE
            }
        })
        .sum()
}

fn effort_label(effort: &str) -> &'static str {
    match effort {
        "low" => "低",
        "medium" => "中",
        "high" => "高",
        "xhigh" => "极高",
        _ => "默认",
    }
}

fn runtime_controls_locked(state: &ZodeAppState) -> bool {
    state
        .current_session
        .as_ref()
        .is_some_and(|session| state.active_turns.contains_key(session))
}

fn is_model_menu(menu: ComposerFooterMenu) -> bool {
    matches!(
        menu,
        ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed
    )
}

fn is_model_disclosure(id: WidgetId) -> bool {
    matches!(
        id,
        COMPOSER_MODEL_MODELS_ID | COMPOSER_MODEL_EFFORTS_ID | COMPOSER_MODEL_SPEEDS_ID
    )
}

fn control_background(
    painter: &mut dyn Painter,
    rect: Rect,
    id: WidgetId,
    active: bool,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if active || focused == Some(id) || hovered == Some(id) {
        painter.fill_round_rect(rect, rect.size.y / 2.0, theme.tokens.muted);
    }
}

fn paint_icon(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    bounds: Rect,
    color: jian_widgets::Color,
    _theme: &ZodeTheme,
) {
    let rect = Rect::xywh(bounds.origin.x + 6.0, bounds.origin.y + 6.0, 16.0, 16.0);
    painter.stroke_svg_path(
        icon.path(),
        rect.origin,
        rect.size.x,
        color,
        icon.stroke_width(),
    );
}
