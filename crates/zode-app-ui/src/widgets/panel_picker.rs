use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, SecondaryPane, ShellRoute, ZodeAppState};
use zode_node_protocol::NodeCapability;

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const PANEL_PICKER_ID: WidgetId = WidgetId(66);
pub const PANEL_PICKER_MENU_ID: WidgetId = WidgetId(67);

const MENU_WIDTH: f32 = 244.0;
const MENU_PADDING: f32 = 5.0;
const ROW_HEIGHT: f32 = 40.0;
const ITEM_BASE: u64 = 68;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelMenuItemLayout {
    pub id: WidgetId,
    pub pane: SecondaryPane,
    pub rect: Rect,
    pub label: &'static str,
    pub icon: SemanticIcon,
    pub enabled: bool,
    pub selected: bool,
    pub unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelPickerMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub items: Vec<PanelMenuItemLayout>,
}

pub struct PanelPicker;

impl PanelPicker {
    pub fn menu_layout(
        anchor: Rect,
        viewport: Rect,
        state: &ZodeAppState,
    ) -> Option<PanelPickerMenuLayout> {
        if !state.presentation.secondary_menu_open
            || state.presentation.route != ShellRoute::Conversation
            || anchor.size.x <= 0.0
            || anchor.size.y <= 0.0
        {
            return None;
        }

        let descriptors = descriptors();
        let desired_height = MENU_PADDING * 2.0 + ROW_HEIGHT * descriptors.len() as f32;
        let height = desired_height.min((viewport.size.y - 16.0).max(0.0));
        let row_height = ((height - MENU_PADDING * 2.0).max(0.0) / descriptors.len().max(1) as f32)
            .min(ROW_HEIGHT);
        let min_x = viewport.origin.x + 8.0;
        let max_x = (viewport.max_x() - MENU_WIDTH - 8.0).max(min_x);
        let x = (anchor.max_x() - MENU_WIDTH).clamp(min_x, max_x);
        let preferred_y = anchor.max_y() + 6.0;
        let min_y = viewport.origin.y + 8.0;
        let max_y = (viewport.max_y() - height - 8.0).max(min_y);
        let y = preferred_y.clamp(min_y, max_y);
        let rect = Rect::xywh(x, y, MENU_WIDTH, height);
        let items = descriptors
            .into_iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let (enabled, reason) = availability(state, descriptor.pane);
                PanelMenuItemLayout {
                    id: WidgetId(ITEM_BASE + index as u64),
                    pane: descriptor.pane,
                    rect: Rect::xywh(
                        rect.origin.x + MENU_PADDING,
                        rect.origin.y + MENU_PADDING + index as f32 * row_height,
                        (rect.size.x - MENU_PADDING * 2.0).max(0.0),
                        row_height,
                    ),
                    label: descriptor.label,
                    icon: descriptor.icon,
                    enabled,
                    selected: state.presentation.secondary_pane == Some(descriptor.pane),
                    unavailable_reason: reason,
                }
            })
            .collect();
        Some(PanelPickerMenuLayout {
            id: PANEL_PICKER_MENU_ID,
            rect,
            items,
        })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == PANEL_PICKER_ID {
            return Some(AppCommand::ToggleSecondaryMenu);
        }
        let index = id.0.checked_sub(ITEM_BASE)? as usize;
        let descriptor = descriptors().get(index).copied()?;
        let (enabled, _) = availability(state, descriptor.pane);
        if !state.presentation.secondary_menu_open || !enabled {
            return None;
        }
        if state.presentation.secondary_pane == Some(descriptor.pane) {
            Some(AppCommand::CloseSecondary)
        } else {
            Some(AppCommand::OpenSecondary(descriptor.pane))
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        layout: &PanelPickerMenuLayout,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        painter.fill_drop_shadow(
            Rect::xywh(
                layout.rect.origin.x,
                layout.rect.origin.y + 2.0,
                layout.rect.size.x,
                layout.rect.size.y,
            ),
            10.0,
            18.0,
            theme.tokens.foreground.with_alpha(0.12),
        );
        painter.fill_round_rect(layout.rect, 11.0, theme.tokens.popover);
        painter.stroke_round_rect(layout.rect, 11.0, theme.tokens.border, 1.0);

        for item in &layout.items {
            if item.enabled && hovered == Some(item.id) {
                painter.fill_round_rect(item.rect, 7.0, theme.tokens.accent);
            }
            if focused == Some(item.id) {
                painter.stroke_round_rect(item.rect, 7.0, theme.tokens.ring, 1.5);
            }
            let foreground = if item.enabled {
                theme.tokens.popover_foreground
            } else {
                theme.tokens.muted_foreground.with_alpha(0.68)
            };
            let icon = Rect::xywh(
                item.rect.origin.x + 10.0,
                item.rect.origin.y + (item.rect.size.y - 16.0) / 2.0,
                16.0,
                16.0,
            );
            painter.stroke_svg_path(
                item.icon.path(),
                icon.origin,
                icon.size.x,
                foreground,
                item.icon.stroke_width(),
            );
            paint_single_line(
                painter,
                item.label,
                Rect::xywh(
                    icon.max_x() + 9.0,
                    item.rect.origin.y,
                    126.0,
                    item.rect.size.y,
                ),
                13.0,
                if item.selected { 600 } else { 400 },
                foreground,
                HorizontalAlign::Start,
            );
            let status = if item.selected {
                Some("已打开")
            } else if !item.enabled {
                Some("不可用")
            } else {
                None
            };
            if let Some(status) = status {
                paint_single_line(
                    painter,
                    status,
                    Rect::xywh(
                        item.rect.max_x() - 62.0,
                        item.rect.origin.y,
                        50.0,
                        item.rect.size.y,
                    ),
                    11.0,
                    400,
                    foreground,
                    HorizontalAlign::End,
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PanelDescriptor {
    pane: SecondaryPane,
    label: &'static str,
    icon: SemanticIcon,
}

fn descriptors() -> [PanelDescriptor; 7] {
    [
        PanelDescriptor {
            pane: SecondaryPane::Environment,
            label: "环境信息",
            icon: SemanticIcon::Environment,
        },
        PanelDescriptor {
            pane: SecondaryPane::Review,
            label: "审查",
            icon: SemanticIcon::ReviewChange,
        },
        PanelDescriptor {
            pane: SecondaryPane::Terminal,
            label: "终端",
            icon: SemanticIcon::Terminal,
        },
        PanelDescriptor {
            pane: SecondaryPane::Browser,
            label: "浏览器",
            icon: SemanticIcon::Browser,
        },
        PanelDescriptor {
            pane: SecondaryPane::Files,
            label: "文件",
            icon: SemanticIcon::Folder,
        },
        PanelDescriptor {
            pane: SecondaryPane::DocumentPreview,
            label: "文档预览",
            icon: SemanticIcon::FileText,
        },
        PanelDescriptor {
            pane: SecondaryPane::SideTask,
            label: "侧边任务",
            icon: SemanticIcon::Chat,
        },
    ]
}

fn availability(state: &ZodeAppState, pane: SecondaryPane) -> (bool, Option<&'static str>) {
    match pane {
        SecondaryPane::Environment | SecondaryPane::Review => {
            let enabled = state.current_session.is_some();
            (enabled, (!enabled).then_some("需要先选择任务"))
        }
        SecondaryPane::Terminal => {
            let capability = state
                .host
                .capabilities
                .capabilities
                .contains(&NodeCapability::Terminal);
            let workspace = state
                .current_session
                .as_ref()
                .and_then(|session| state.available_workspace_for_session(session))
                .or_else(|| state.active_available_workspace())
                .or(state.projectless_workspace_root.as_ref())
                .is_some();
            let enabled = capability && workspace;
            (
                enabled,
                (!enabled).then_some(if capability {
                    "当前任务没有工作目录"
                } else {
                    "当前节点不支持终端"
                }),
            )
        }
        SecondaryPane::DocumentPreview => {
            let enabled = state
                .current_session_presentation()
                .and_then(|presentation| presentation.preview.target())
                .is_some();
            (enabled, (!enabled).then_some("尚未打开文档"))
        }
        SecondaryPane::Browser => (false, Some("浏览器会话尚未接入桌面端")),
        SecondaryPane::Files => (false, Some("文件树查询尚未接入桌面端")),
        SecondaryPane::SideTask => (false, Some("侧边任务尚未接入桌面端")),
    }
}

#[cfg(test)]
mod tests {
    use super::{availability, PANEL_PICKER_ID};
    use zode_app_model::{demo_state, SecondaryPane};

    #[test]
    fn unavailable_typed_panes_never_emit_fake_actions() {
        let state = demo_state();
        for pane in [
            SecondaryPane::Browser,
            SecondaryPane::Files,
            SecondaryPane::SideTask,
        ] {
            assert!(!availability(&state, pane).0);
        }
        assert_eq!(PANEL_PICKER_ID.0, 66);
    }
}
