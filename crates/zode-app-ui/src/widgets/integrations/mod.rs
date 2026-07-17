mod catalog;
mod installed;
mod row;

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, IntegrationsTab, LoadState, ShellRoute, ZodeAppState};

use crate::{paint_single_line, SemanticIcon, WidgetId, ZodeTheme};

pub use catalog::CatalogSectionLayout;
pub use installed::InstalledIconLayout;
pub use row::IntegrationRowLayout;

pub const INTEGRATIONS_PLUGINS_TAB_ID: WidgetId = WidgetId(70);
pub const INTEGRATIONS_SKILLS_TAB_ID: WidgetId = WidgetId(71);

const CONTENT_WIDTH: f32 = 736.0;
const CONTENT_TOP: f32 = 46.0;
const TAB_TOP: f32 = 6.0;
const TAB_WIDTH: f32 = 48.0;
const TAB_HEIGHT: f32 = 32.0;
const TAB_GAP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrationTabLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub tab: IntegrationsTab,
    pub label: &'static str,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegrationsPageLayout {
    pub content: Rect,
    pub title: Rect,
    pub subtitle: Rect,
    pub search: Rect,
    pub installed_title: Rect,
    pub installed_strip: Rect,
    pub directory_status: Rect,
    pub catalog: Rect,
    pub tabs: [IntegrationTabLayout; 2],
}

pub struct IntegrationsPage;

impl IntegrationsPage {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        let ShellRoute::Integrations(tab) = state.presentation.route else {
            return;
        };
        let layout = Self::layout(rect, state);
        painter.save();
        painter.clip_rect(rect);
        paint_tabs(painter, &layout, theme);
        paint_header(painter, &layout, tab, theme);
        match &state.presentation.integrations {
            LoadState::Ready(catalog) => {
                installed::paint(painter, &layout, state, theme);
                catalog::paint(painter, &layout, state, catalog, theme);
            }
            LoadState::Idle => paint_load_state(painter, &layout, "尚未加载本机集成", theme),
            LoadState::Loading => paint_load_state(painter, &layout, "正在读取本机集成…", theme),
            LoadState::Failed(message) => paint_load_state(painter, &layout, message, theme),
        }
        painter.restore();
    }

    pub fn layout(rect: Rect, state: &ZodeAppState) -> IntegrationsPageLayout {
        let content_width = rect.size.x.clamp(0.0, CONTENT_WIDTH);
        let content = Rect::xywh(
            rect.origin.x + (rect.size.x - content_width).max(0.0) / 2.0,
            rect.origin.y + CONTENT_TOP,
            content_width,
            (rect.size.y - CONTENT_TOP).max(0.0),
        );
        let selected = match state.presentation.route {
            ShellRoute::Integrations(tab) => tab,
            _ => IntegrationsTab::Plugins,
        };
        let tab_x = rect.origin.x + 8.0;
        let tabs = [
            IntegrationTabLayout {
                id: INTEGRATIONS_PLUGINS_TAB_ID,
                rect: Rect::xywh(tab_x, rect.origin.y + TAB_TOP, TAB_WIDTH, TAB_HEIGHT),
                tab: IntegrationsTab::Plugins,
                label: "插件",
                selected: selected == IntegrationsTab::Plugins,
            },
            IntegrationTabLayout {
                id: INTEGRATIONS_SKILLS_TAB_ID,
                rect: Rect::xywh(
                    tab_x + TAB_WIDTH + TAB_GAP,
                    rect.origin.y + TAB_TOP,
                    TAB_WIDTH,
                    TAB_HEIGHT,
                ),
                tab: IntegrationsTab::Skills,
                label: "技能",
                selected: selected == IntegrationsTab::Skills,
            },
        ];

        IntegrationsPageLayout {
            title: Rect::xywh(
                content.origin.x,
                content.origin.y + 12.0,
                content.size.x,
                36.0,
            ),
            subtitle: Rect::xywh(
                content.origin.x,
                content.origin.y + 48.0,
                content.size.x,
                24.0,
            ),
            search: Rect::xywh(
                content.origin.x,
                content.origin.y + 100.0,
                content.size.x,
                34.0,
            ),
            installed_title: Rect::xywh(
                content.origin.x,
                content.origin.y + 158.0,
                content.size.x,
                24.0,
            ),
            installed_strip: Rect::xywh(
                content.origin.x,
                content.origin.y + 190.0,
                content.size.x,
                44.0,
            ),
            directory_status: Rect::xywh(
                content.origin.x,
                content.origin.y + 246.0,
                content.size.x,
                22.0,
            ),
            catalog: Rect::xywh(
                content.origin.x,
                content.origin.y + 282.0,
                content.size.x,
                (content.size.y - 282.0).max(0.0),
            ),
            content,
            tabs,
        }
    }

    pub fn installed_icon_layout(rect: Rect, state: &ZodeAppState) -> Vec<InstalledIconLayout> {
        installed::layout(&Self::layout(rect, state), state)
    }

    pub fn catalog_section_layout(rect: Rect, state: &ZodeAppState) -> Vec<CatalogSectionLayout> {
        catalog::layout(&Self::layout(rect, state), state)
    }

    pub fn row_widget_id(source_id: &str) -> WidgetId {
        row::widget_id(source_id)
    }

    pub const fn command_for_widget(id: WidgetId) -> Option<AppCommand> {
        match id {
            INTEGRATIONS_PLUGINS_TAB_ID => {
                Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Plugins))
            }
            INTEGRATIONS_SKILLS_TAB_ID => {
                Some(AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills))
            }
            _ => None,
        }
    }
}

fn paint_tabs(painter: &mut dyn Painter, layout: &IntegrationsPageLayout, theme: &ZodeTheme) {
    for tab in layout.tabs {
        if tab.selected {
            painter.fill_round_rect(tab.rect, 10.0, theme.tokens.row_selected);
        }
        paint_single_line(
            painter,
            tab.label,
            tab.rect,
            13.0,
            if tab.selected { 650 } else { 450 },
            if tab.selected {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Center,
        );
    }
}

fn paint_header(
    painter: &mut dyn Painter,
    layout: &IntegrationsPageLayout,
    tab: IntegrationsTab,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        match tab {
            IntegrationsTab::Plugins => "插件",
            IntegrationsTab::Skills => "技能",
        },
        layout.title,
        28.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        match tab {
            IntegrationsTab::Plugins => "在常用工具与 Zode 协作",
            IntegrationsTab::Skills => "通过可复用指令扩展 Zode 工作流",
        },
        layout.subtitle,
        15.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );

    painter.fill_round_rect(layout.search, 17.0, theme.tokens.muted);
    painter.stroke_round_rect(layout.search, 17.0, theme.tokens.border, 1.0);
    let icon_size = 16.0;
    painter.stroke_svg_path(
        SemanticIcon::Search.path(),
        jian_widgets::Point2D::new(
            layout.search.origin.x + 12.0,
            layout.search.origin.y + (layout.search.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.tokens.muted_foreground,
        SemanticIcon::Search.stroke_width(),
    );
    paint_single_line(
        painter,
        "搜索本机集成（即将支持）",
        Rect::xywh(
            layout.search.origin.x + 38.0,
            layout.search.origin.y,
            (layout.search.size.x - 50.0).max(0.0),
            layout.search.size.y,
        ),
        13.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

fn paint_load_state(
    painter: &mut dyn Painter,
    layout: &IntegrationsPageLayout,
    message: &str,
    theme: &ZodeTheme,
) {
    let panel = Rect::xywh(
        layout.content.origin.x,
        layout.content.origin.y + 158.0,
        layout.content.size.x,
        112.0,
    );
    painter.fill_round_rect(panel, 12.0, theme.tokens.card);
    painter.stroke_round_rect(panel, 12.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        message,
        Rect::xywh(
            panel.origin.x + 16.0,
            panel.origin.y,
            (panel.size.x - 32.0).max(0.0),
            panel.size.y,
        ),
        14.0,
        500,
        theme.tokens.muted_foreground,
        HorizontalAlign::Center,
    );
}
