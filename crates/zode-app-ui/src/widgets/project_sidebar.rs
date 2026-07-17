use jian_widgets::{centered_text_baseline_y, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{ShellPage, ZodeAppState};

use crate::ZodeTheme;

const FONT: &str = "system-ui";
const HEADER_H: f32 = 46.0;
const ROW_H: f32 = 32.0;
const ROW_INSET: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    NewSession,
    Navigate {
        page: ShellPage,
        feature: Option<&'static str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarItem {
    pub label: &'static str,
    pub action: SidebarAction,
}

const NAVIGATION: [SidebarItem; 6] = [
    SidebarItem {
        label: "新建任务",
        action: SidebarAction::NewSession,
    },
    SidebarItem {
        label: "工作流",
        action: SidebarAction::Navigate {
            page: ShellPage::ComingSoon,
            feature: Some("工作流"),
        },
    },
    SidebarItem {
        label: "插件",
        action: SidebarAction::Navigate {
            page: ShellPage::ComingSoon,
            feature: Some("插件"),
        },
    },
    SidebarItem {
        label: "OpenPencil",
        action: SidebarAction::Navigate {
            page: ShellPage::ComingSoon,
            feature: Some("OpenPencil"),
        },
    },
    SidebarItem {
        label: "浏览器",
        action: SidebarAction::Navigate {
            page: ShellPage::ComingSoon,
            feature: Some("浏览器"),
        },
    },
    SidebarItem {
        label: "账户与设置",
        action: SidebarAction::Navigate {
            page: ShellPage::Settings,
            feature: None,
        },
    },
];

pub struct ProjectSidebar;

impl ProjectSidebar {
    pub const fn navigation_items() -> &'static [SidebarItem] {
        &NAVIGATION
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        draw_label(
            painter,
            "ZODE",
            Rect::xywh(
                rect.origin.x + ROW_INSET,
                rect.origin.y,
                rect.size.x - ROW_INSET,
                HEADER_H,
            ),
            13.0,
            700,
            theme.zode_purple,
        );

        let compact = rect.size.x < 100.0;
        for (index, item) in NAVIGATION.iter().enumerate() {
            let row = Rect::xywh(
                rect.origin.x + 8.0,
                rect.origin.y + HEADER_H + index as f32 * ROW_H,
                (rect.size.x - 16.0).max(0.0),
                ROW_H,
            );
            if index == 0 {
                painter.fill_round_rect(
                    row,
                    theme.tokens.radius,
                    theme.tokens.row_selected_primary,
                );
            }
            let label = if compact {
                item.label.chars().next().unwrap_or(' ').to_string()
            } else {
                item.label.to_string()
            };
            draw_label(
                painter,
                &label,
                Rect::xywh(
                    row.origin.x + 8.0,
                    row.origin.y,
                    (row.size.x - 16.0).max(0.0),
                    row.size.y,
                ),
                13.0,
                if index == 0 { 600 } else { 400 },
                theme.sidebar_foreground,
            );
        }

        if !compact {
            let project_y = rect.origin.y + HEADER_H + NAVIGATION.len() as f32 * ROW_H + 18.0;
            draw_label(
                painter,
                if state.projects.is_empty() {
                    "项目"
                } else {
                    "最近项目"
                },
                Rect::xywh(rect.origin.x + 16.0, project_y, rect.size.x - 32.0, 24.0),
                11.0,
                600,
                theme.tokens.muted_foreground,
            );
            for (index, thread) in state.threads.iter().take(8).enumerate() {
                draw_label(
                    painter,
                    &thread.title,
                    Rect::xywh(
                        rect.origin.x + 16.0,
                        project_y + 28.0 + index as f32 * ROW_H,
                        rect.size.x - 32.0,
                        ROW_H,
                    ),
                    12.0,
                    400,
                    theme.sidebar_foreground,
                );
            }
        }
    }
}

fn draw_label(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, FONT, size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(
        &layout,
        Point2D::new(rect.origin.x, centered_text_baseline_y(rect, size)),
    );
}
