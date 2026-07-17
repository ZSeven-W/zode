use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, SecondaryPane, ZodeAppState};
use zode_node_protocol::NodeCapability;

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const PANEL_PICKER_ID: WidgetId = WidgetId(66);
pub const SECONDARY_HOME_REVIEW_ID: WidgetId = WidgetId(240);
pub const SECONDARY_HOME_TERMINAL_ID: WidgetId = WidgetId(241);
pub const SECONDARY_HOME_BROWSER_ID: WidgetId = WidgetId(242);
pub const SECONDARY_HOME_FILES_ID: WidgetId = WidgetId(243);
pub const SECONDARY_HOME_SIDE_TASK_ID: WidgetId = WidgetId(244);

const HOME_WIDTH: f32 = 420.0;
const HOME_ROW_HEIGHT: f32 = 46.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPickerHomeItemLayout {
    pub id: WidgetId,
    pub pane: SecondaryPane,
    pub rect: Rect,
    pub label: &'static str,
    pub shortcut: Option<&'static str>,
    pub icon: SemanticIcon,
    pub enabled: bool,
    pub unavailable_reason: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelPickerHomeLayout {
    pub rect: Rect,
    pub items: Vec<PanelPickerHomeItemLayout>,
}

pub struct PanelPicker;

impl PanelPicker {
    pub fn home_layout(rect: Rect, state: &ZodeAppState) -> Option<PanelPickerHomeLayout> {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return None;
        }
        let descriptors = descriptors();
        let width = HOME_WIDTH.min((rect.size.x - 32.0).max(0.0));
        let height = HOME_ROW_HEIGHT * descriptors.len() as f32;
        let x = rect.origin.x + (rect.size.x - width).max(0.0) / 2.0;
        let y = rect.origin.y + (rect.size.y - height).max(0.0) / 2.0;
        let items = descriptors
            .into_iter()
            .enumerate()
            .map(|(index, descriptor)| {
                let (enabled, unavailable_reason) = availability(state, descriptor.pane);
                PanelPickerHomeItemLayout {
                    id: home_item_id(descriptor.pane),
                    pane: descriptor.pane,
                    rect: Rect::xywh(
                        x,
                        y + index as f32 * HOME_ROW_HEIGHT,
                        width,
                        HOME_ROW_HEIGHT,
                    ),
                    label: descriptor.label,
                    shortcut: descriptor.shortcut,
                    icon: descriptor.icon,
                    enabled,
                    unavailable_reason,
                }
            })
            .collect();
        Some(PanelPickerHomeLayout { rect, items })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == PANEL_PICKER_ID {
            return Some(AppCommand::ToggleSidebar);
        }
        if let Some(pane) = home_pane_for_id(id) {
            let (enabled, _) = availability(state, pane);
            return (state.presentation.secondary_sidebar_open && enabled)
                .then_some(AppCommand::OpenSecondary(pane));
        }
        None
    }

    pub fn paint_home(
        painter: &mut dyn Painter,
        layout: &PanelPickerHomeLayout,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        painter.fill_rect(layout.rect, theme.tokens.background);
        for item in &layout.items {
            if item.enabled && (hovered == Some(item.id) || focused == Some(item.id)) {
                painter.fill_round_rect(item.rect, 8.0, theme.tokens.accent);
            }
            let foreground = if item.enabled {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground.with_alpha(0.68)
            };
            let icon = Rect::xywh(
                item.rect.origin.x + 12.0,
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
                    icon.max_x() + 10.0,
                    item.rect.origin.y,
                    (item.rect.size.x - 112.0).max(0.0),
                    item.rect.size.y,
                ),
                13.0,
                400,
                foreground,
                HorizontalAlign::Start,
            );
            let trailing = if item.enabled {
                item.shortcut
            } else {
                Some("不可用")
            };
            if let Some(trailing) = trailing {
                paint_single_line(
                    painter,
                    trailing,
                    Rect::xywh(
                        item.rect.max_x() - 70.0,
                        item.rect.origin.y,
                        58.0,
                        item.rect.size.y,
                    ),
                    11.0,
                    400,
                    theme.tokens.muted_foreground,
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
    shortcut: Option<&'static str>,
}

fn descriptors() -> [PanelDescriptor; 5] {
    [
        PanelDescriptor {
            pane: SecondaryPane::Review,
            label: "审阅",
            icon: SemanticIcon::ReviewChange,
            shortcut: Some("⌃⇧G"),
        },
        PanelDescriptor {
            pane: SecondaryPane::Terminal,
            label: "终端",
            icon: SemanticIcon::Terminal,
            shortcut: None,
        },
        PanelDescriptor {
            pane: SecondaryPane::Browser,
            label: "浏览器",
            icon: SemanticIcon::Browser,
            shortcut: Some("⌘T"),
        },
        PanelDescriptor {
            pane: SecondaryPane::Files,
            label: "文件",
            icon: SemanticIcon::Folder,
            shortcut: Some("⌘P"),
        },
        PanelDescriptor {
            pane: SecondaryPane::SideTask,
            label: "侧边任务",
            icon: SemanticIcon::Chat,
            shortcut: Some("⌥⌘S"),
        },
    ]
}

const fn home_item_id(pane: SecondaryPane) -> WidgetId {
    match pane {
        SecondaryPane::Review => SECONDARY_HOME_REVIEW_ID,
        SecondaryPane::Terminal => SECONDARY_HOME_TERMINAL_ID,
        SecondaryPane::Browser => SECONDARY_HOME_BROWSER_ID,
        SecondaryPane::Files => SECONDARY_HOME_FILES_ID,
        SecondaryPane::SideTask => SECONDARY_HOME_SIDE_TASK_ID,
        // `Subagents` is opened only from the environment card's compact
        // Subagents row (see `EnvironmentPanel::command_for_widget`), never
        // from this home grid - like `Environment`/`DocumentPreview`, it has
        // no tile here.
        SecondaryPane::Environment | SecondaryPane::DocumentPreview | SecondaryPane::Subagents => {
            WidgetId(0)
        }
    }
}

const fn home_pane_for_id(id: WidgetId) -> Option<SecondaryPane> {
    match id {
        SECONDARY_HOME_REVIEW_ID => Some(SecondaryPane::Review),
        SECONDARY_HOME_TERMINAL_ID => Some(SecondaryPane::Terminal),
        SECONDARY_HOME_BROWSER_ID => Some(SecondaryPane::Browser),
        SECONDARY_HOME_FILES_ID => Some(SecondaryPane::Files),
        SECONDARY_HOME_SIDE_TASK_ID => Some(SecondaryPane::SideTask),
        _ => None,
    }
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
        SecondaryPane::Browser => {
            // Mirrors `Terminal`: the row-level gate is the host capability
            // manifest, not the (possibly per-launch, dynamic) reason text
            // in `state.browser` - that field carries the specific message
            // `BrowserPanel` shows once the panel is actually open.
            let enabled = state
                .host
                .capabilities
                .capabilities
                .contains(&NodeCapability::Browser);
            (enabled, (!enabled).then_some("当前节点不支持浏览器"))
        }
        SecondaryPane::Files => (false, Some("文件树查询尚未接入桌面端")),
        SecondaryPane::SideTask => (false, Some("侧边任务尚未接入桌面端")),
        // Never rendered as a picker row (see `home_item_id`'s doc comment);
        // this arm exists only to keep the match exhaustive.
        SecondaryPane::Subagents => (false, None),
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
