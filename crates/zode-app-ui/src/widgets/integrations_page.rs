use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, IntegrationsTab, ShellRoute, ZodeAppState};
use zode_node_protocol::NodeCapability;

use crate::{paint_single_line, WidgetId, ZodeTheme};

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
    pub search: Rect,
    pub tabs: [IntegrationTabLayout; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityCard {
    pub capability: NodeCapability,
    pub label: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityCardLayout {
    pub card: CapabilityCard,
    pub rect: Rect,
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
        match tab {
            IntegrationsTab::Plugins => paint_plugins(painter, rect, &layout, state, theme),
            IntegrationsTab::Skills => paint_skills(painter, &layout, theme),
        }
        painter.restore();
    }

    pub fn capability_cards(state: &ZodeAppState) -> Vec<CapabilityCard> {
        state
            .host
            .capabilities
            .capabilities
            .iter()
            .cloned()
            .map(|capability| {
                let (label, description) = capability_copy(&capability);
                CapabilityCard {
                    capability,
                    label,
                    description,
                }
            })
            .collect()
    }

    pub fn capability_card_layout(rect: Rect, state: &ZodeAppState) -> Vec<CapabilityCardLayout> {
        if state.presentation.route != ShellRoute::Integrations(IntegrationsTab::Plugins) {
            return Vec::new();
        }
        let content = Self::layout(rect, state).content;
        let column_count = if content.size.x < 600.0 { 1 } else { 2 };
        let column_gap = if column_count == 1 {
            0.0
        } else {
            32.0_f32.min(content.size.x)
        };
        let card_width = ((content.size.x - column_gap) / column_count as f32).max(0.0);
        let cards = Self::capability_cards(state);
        let row_count = cards.len().div_ceil(column_count);
        let cards_top = content.origin.y + 206.0;
        let available_height = (content.origin.y + content.size.y - cards_top).max(0.0);
        let natural_height = row_count as f32 * 74.0 + row_count.saturating_sub(1) as f32 * 12.0;
        let (card_height, row_gap) = if row_count > 0 && natural_height > available_height {
            let row_gap = 6.0;
            let compact_height = (available_height - row_count.saturating_sub(1) as f32 * row_gap)
                / row_count as f32;
            (compact_height.clamp(48.0, 74.0), row_gap)
        } else {
            (74.0, 12.0)
        };
        cards
            .into_iter()
            .enumerate()
            .map(|(index, card)| CapabilityCardLayout {
                card,
                rect: Rect::xywh(
                    content.origin.x + (index % column_count) as f32 * (card_width + column_gap),
                    cards_top + (index / column_count) as f32 * (card_height + row_gap),
                    card_width,
                    card_height,
                ),
            })
            .collect()
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
            content,
            search: Rect::xywh(
                content.origin.x,
                content.origin.y + 100.0,
                content.size.x,
                34.0,
            ),
            tabs,
        }
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

fn paint_plugins(
    painter: &mut dyn Painter,
    rect: Rect,
    layout: &IntegrationsPageLayout,
    state: &ZodeAppState,
    theme: &ZodeTheme,
) {
    let content = layout.content;
    draw_text(
        painter,
        "插件",
        Point2D::new(content.origin.x, content.origin.y + 42.0),
        28.0,
        650,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        "使用当前节点提供的本地能力",
        Point2D::new(content.origin.x, content.origin.y + 74.0),
        15.0,
        400,
        theme.tokens.muted_foreground,
    );
    paint_search_status(painter, layout.search, theme);
    draw_text(
        painter,
        "可用能力",
        Point2D::new(content.origin.x, content.origin.y + 184.0),
        16.0,
        600,
        theme.tokens.foreground,
    );

    let cards = IntegrationsPage::capability_card_layout(rect, state);
    if cards.is_empty() {
        draw_text(
            painter,
            "当前节点尚未提供能力",
            Point2D::new(content.origin.x, content.origin.y + 240.0),
            14.0,
            400,
            theme.tokens.muted_foreground,
        );
    }
    for card in cards {
        painter.fill_round_rect(card.rect, 12.0, theme.tokens.card);
        painter.stroke_round_rect(card.rect, 12.0, theme.tokens.border, 1.0);
        painter.save();
        painter.clip_round_rect(card.rect, 12.0);
        let icon_size = (card.rect.size.y - 16.0).clamp(28.0, 40.0);
        let icon = Rect::xywh(
            card.rect.origin.x + 14.0,
            card.rect.origin.y + (card.rect.size.y - icon_size) / 2.0,
            icon_size,
            icon_size,
        );
        painter.fill_round_rect(icon, 10.0, theme.tokens.muted);
        let card_center_y = card.rect.origin.y + card.rect.size.y / 2.0;
        let text_rect = Rect::xywh(
            card.rect.origin.x + 66.0,
            card_center_y - 20.0,
            (card.rect.size.x - 80.0).max(0.0),
            20.0,
        );
        paint_single_line(
            painter,
            card.card.label,
            text_rect,
            14.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            card.card.description,
            Rect::xywh(text_rect.origin.x, card_center_y, text_rect.size.x, 20.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        painter.restore();
    }
}

fn paint_skills(painter: &mut dyn Painter, layout: &IntegrationsPageLayout, theme: &ZodeTheme) {
    let content = layout.content;
    draw_text(
        painter,
        "技能",
        Point2D::new(content.origin.x, content.origin.y + 42.0),
        28.0,
        650,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        "通过可复用指令扩展 Zode 工作流",
        Point2D::new(content.origin.x, content.origin.y + 74.0),
        15.0,
        400,
        theme.tokens.muted_foreground,
    );
    paint_search_status(painter, layout.search, theme);

    let empty = Rect::xywh(
        content.origin.x,
        content.origin.y + 206.0,
        content.size.x,
        136.0,
    );
    painter.fill_round_rect(empty, 12.0, theme.tokens.card);
    painter.stroke_round_rect(empty, 12.0, theme.tokens.border, 1.0);
    draw_centered_text(
        painter,
        "尚未接入技能目录",
        empty,
        empty.origin.y + 62.0,
        15.0,
        600,
        theme.tokens.foreground,
    );
    draw_centered_text(
        painter,
        "接入真实目录后，这里将展示可用技能。",
        empty,
        empty.origin.y + 88.0,
        13.0,
        400,
        theme.tokens.muted_foreground,
    );
}

fn paint_search_status(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 17.0, theme.tokens.muted);
    painter.stroke_round_rect(rect, 17.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        "搜索即将支持",
        Rect::xywh(
            rect.origin.x + 14.0,
            rect.origin.y,
            (rect.size.x - 28.0).max(0.0),
            rect.size.y,
        ),
        13.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

fn draw_centered_text(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    baseline_y: f32,
    preferred_size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let preferred_width = painter.measure_text_weighted(text, preferred_size, weight);
    let available_width = (rect.size.x - 24.0).max(0.0);
    let font_size = if preferred_width > available_width && preferred_width > 0.0 {
        preferred_size * available_width / preferred_width
    } else {
        preferred_size
    };
    let width = if font_size == preferred_size {
        preferred_width
    } else {
        painter.measure_text_weighted(text, font_size, weight)
    };
    draw_text(
        painter,
        text,
        Point2D::new(
            rect.origin.x + (rect.size.x - width).max(0.0) / 2.0,
            baseline_y,
        ),
        font_size,
        weight,
        color,
    );
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

const fn capability_copy(capability: &NodeCapability) -> (&'static str, &'static str) {
    match capability {
        NodeCapability::Agent => ("智能体", "运行并协调 AI 编码任务"),
        NodeCapability::Workspace => ("工作区", "读取当前项目与会话上下文"),
        NodeCapability::FileSystem => ("文件系统", "读取和修改本地文件"),
        NodeCapability::Terminal => ("终端", "运行本地命令与开发工具"),
        NodeCapability::Browser => ("浏览器", "打开网页并与页面交互"),
        NodeCapability::Camera => ("相机", "访问设备摄像头输入"),
        NodeCapability::Notifications => ("通知", "发送本地系统通知"),
        NodeCapability::Approval => ("审批", "在敏感操作前请求确认"),
    }
}
