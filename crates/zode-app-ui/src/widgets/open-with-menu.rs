use jian_widgets::{Color, HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, ExternalApplication, LoadState, ZodeAppState};

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const OPEN_WITH_PRIMARY_ID: WidgetId = WidgetId(210);
pub const OPEN_WITH_DROPDOWN_ID: WidgetId = WidgetId(211);
pub const OPEN_WITH_MENU_ID: WidgetId = WidgetId(212);
const OPEN_WITH_ITEM_BASE: u64 = 213;
const MENU_WIDTH: f32 = 224.0;
const MENU_PADDING: f32 = 5.0;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenWithSplitLayout {
    pub rect: Rect,
    pub primary: Rect,
    pub dropdown: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenWithMenuItemLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub application: ExternalApplication,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenWithMenuLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub items: Vec<OpenWithMenuItemLayout>,
    pub status: Option<&'static str>,
}

pub struct OpenWithMenu;

impl OpenWithMenu {
    pub fn split_layout(rect: Rect) -> OpenWithSplitLayout {
        let primary_width = (rect.size.x - 23.0).max(0.0);
        OpenWithSplitLayout {
            rect,
            primary: Rect::xywh(rect.origin.x, rect.origin.y, primary_width, rect.size.y),
            dropdown: Rect::xywh(
                rect.origin.x + primary_width,
                rect.origin.y,
                (rect.size.x - primary_width).max(0.0),
                rect.size.y,
            ),
        }
    }

    pub fn menu_layout(
        anchor: Rect,
        viewport: Rect,
        state: &ZodeAppState,
    ) -> Option<OpenWithMenuLayout> {
        if !state.open_with.menu_open || current_local_workspace(state).is_none() {
            return None;
        }
        let (applications, status) = match &state.open_with.applications {
            LoadState::Ready(applications) => (
                applications.as_slice(),
                applications.is_empty().then_some("未发现支持的本机应用"),
            ),
            LoadState::Idle | LoadState::Loading => (&[][..], Some("正在扫描本机应用…")),
            LoadState::Failed(_) => (&[][..], Some("未能扫描本机应用")),
        };
        let row_count = applications.len().max(1);
        let height = MENU_PADDING * 2.0 + ROW_HEIGHT * row_count as f32;
        let min_x = viewport.origin.x + 8.0;
        let max_x = (viewport.max_x() - MENU_WIDTH - 8.0).max(min_x);
        let x = (anchor.max_x() - MENU_WIDTH).clamp(min_x, max_x);
        let min_y = viewport.origin.y + 8.0;
        let max_y = (viewport.max_y() - height - 8.0).max(min_y);
        let y = (anchor.max_y() + 6.0).clamp(min_y, max_y);
        let rect = Rect::xywh(x, y, MENU_WIDTH, height);
        let items = applications
            .iter()
            .copied()
            .enumerate()
            .map(|(index, application)| OpenWithMenuItemLayout {
                id: WidgetId(OPEN_WITH_ITEM_BASE + index as u64),
                rect: Rect::xywh(
                    rect.origin.x + MENU_PADDING,
                    rect.origin.y + MENU_PADDING + ROW_HEIGHT * index as f32,
                    rect.size.x - MENU_PADDING * 2.0,
                    ROW_HEIGHT,
                ),
                application,
            })
            .collect();
        Some(OpenWithMenuLayout {
            id: OPEN_WITH_MENU_ID,
            rect,
            items,
            status,
        })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == OPEN_WITH_DROPDOWN_ID {
            return current_local_workspace(state)
                .is_some()
                .then_some(AppCommand::ToggleOpenWithMenu);
        }
        if id == OPEN_WITH_PRIMARY_ID {
            return current_local_workspace(state)
                .cloned()
                .map(|workspace_uri| AppCommand::OpenWorkspaceExternally {
                    workspace_uri,
                    application: state.open_with.primary_application(),
                });
        }
        let index = id.0.checked_sub(OPEN_WITH_ITEM_BASE)? as usize;
        if !state.open_with.menu_open {
            return None;
        }
        let application = state.open_with.applications.ready()?.get(index).copied()?;
        current_local_workspace(state)
            .cloned()
            .map(|workspace_uri| AppCommand::OpenWorkspaceExternally {
                workspace_uri,
                application,
            })
    }

    pub fn paint_trigger(
        painter: &mut dyn Painter,
        layout: OpenWithSplitLayout,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) {
        painter.fill_round_rect(layout.rect, 9.0, theme.tokens.secondary);
        painter.stroke_round_rect(layout.rect, 9.0, theme.tokens.border, 1.0);
        painter.stroke_line(
            Point2D::new(layout.dropdown.origin.x, layout.rect.origin.y + 7.0),
            Point2D::new(layout.dropdown.origin.x, layout.rect.max_y() - 7.0),
            theme.tokens.border,
            1.0,
        );
        let (icon, color) = application_visual(state.open_with.primary_application());
        let icon_rect = centered_square(layout.primary, 16.0);
        painter.stroke_svg_path(
            icon.path(),
            icon_rect.origin,
            icon_rect.size.x,
            color,
            icon.stroke_width(),
        );
        let chevron = centered_square(layout.dropdown, 14.0);
        painter.stroke_svg_path(
            SemanticIcon::ChevronDown.path(),
            chevron.origin,
            chevron.size.x,
            theme.tokens.muted_foreground,
            SemanticIcon::ChevronDown.stroke_width(),
        );
    }

    pub fn paint_menu(
        painter: &mut dyn Painter,
        layout: &OpenWithMenuLayout,
        state: &ZodeAppState,
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
        if let Some(status) = layout.status {
            paint_single_line(
                painter,
                status,
                Rect::xywh(
                    layout.rect.origin.x + 14.0,
                    layout.rect.origin.y + MENU_PADDING,
                    layout.rect.size.x - 28.0,
                    ROW_HEIGHT,
                ),
                12.0,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        for item in &layout.items {
            if hovered == Some(item.id) || focused == Some(item.id) {
                painter.fill_round_rect(item.rect, 7.0, theme.tokens.accent);
            }
            let (icon, color) = application_visual(item.application);
            let icon_background = Rect::xywh(
                item.rect.origin.x + 8.0,
                item.rect.origin.y + (item.rect.size.y - 20.0) / 2.0,
                20.0,
                20.0,
            );
            painter.fill_round_rect(icon_background, 5.0, color.with_alpha(0.13));
            let icon_rect = centered_square(icon_background, 14.0);
            painter.stroke_svg_path(
                icon.path(),
                icon_rect.origin,
                icon_rect.size.x,
                color,
                icon.stroke_width(),
            );
            paint_single_line(
                painter,
                item.application.label(),
                Rect::xywh(
                    icon_background.max_x() + 9.0,
                    item.rect.origin.y,
                    item.rect.size.x - 64.0,
                    item.rect.size.y,
                ),
                13.0,
                400,
                theme.tokens.popover_foreground,
                HorizontalAlign::Start,
            );
            if state.open_with.primary_application() == item.application {
                let check = centered_square(
                    Rect::xywh(
                        item.rect.max_x() - 30.0,
                        item.rect.origin.y,
                        22.0,
                        item.rect.size.y,
                    ),
                    14.0,
                );
                painter.stroke_svg_path(
                    SemanticIcon::Check.path(),
                    check.origin,
                    check.size.x,
                    theme.tokens.muted_foreground,
                    SemanticIcon::Check.stroke_width(),
                );
            }
        }
    }

    pub fn focusable_ids(state: &ZodeAppState) -> Vec<WidgetId> {
        if !state.open_with.menu_open {
            return Vec::new();
        }
        state
            .open_with
            .applications
            .ready()
            .map(|applications| {
                (0..applications.len())
                    .map(|index| WidgetId(OPEN_WITH_ITEM_BASE + index as u64))
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub fn current_local_workspace(state: &ZodeAppState) -> Option<&zode_node_protocol::WorkspaceUri> {
    state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .or_else(|| state.active_available_workspace())
        .filter(|workspace| workspace.as_str().starts_with("file://"))
        .filter(|workspace| !state.is_projectless_workspace(workspace))
}

fn centered_square(rect: Rect, size: f32) -> Rect {
    let size = size.min(rect.size.x.max(0.0)).min(rect.size.y.max(0.0));
    Rect::xywh(
        rect.origin.x + (rect.size.x - size) / 2.0,
        rect.origin.y + (rect.size.y - size) / 2.0,
        size,
        size,
    )
}

fn application_visual(application: ExternalApplication) -> (SemanticIcon, Color) {
    match application {
        ExternalApplication::VisualStudioCode => {
            (SemanticIcon::ExploreCode, Color::rgb_u8(0, 122, 204))
        }
        ExternalApplication::Cursor => (SemanticIcon::Computer, Color::rgb_u8(66, 66, 66)),
        ExternalApplication::Zed => (SemanticIcon::BuildFeature, Color::rgb_u8(82, 82, 82)),
        ExternalApplication::Finder => (SemanticIcon::Folder, Color::rgb_u8(48, 142, 255)),
        ExternalApplication::Terminal => (SemanticIcon::Terminal, Color::rgb_u8(79, 79, 79)),
        ExternalApplication::ITerm2 => (SemanticIcon::Environment, Color::rgb_u8(65, 53, 94)),
        ExternalApplication::Warp => (SemanticIcon::Sparkles, Color::rgb_u8(86, 49, 157)),
        ExternalApplication::Xcode => (SemanticIcon::BuildFeature, Color::rgb_u8(20, 145, 238)),
        ExternalApplication::AndroidStudio => {
            (SemanticIcon::Configuration, Color::rgb_u8(61, 186, 122))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenWithMenu, OPEN_WITH_DROPDOWN_ID, OPEN_WITH_PRIMARY_ID};
    use crate::{Insets, RectExt};
    use zode_app_model::{demo_state, AppCommand, ExternalApplication, LoadState, ProjectState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    fn state_with_workspace() -> zode_app_model::ZodeAppState {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        let session = SessionLocator::new(state.host.node_id, "open-with-layout");
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace.clone(),
            title: "Open with layout".into(),
            updated_at_ms: 1,
            status: ThreadStatus::Idle,
        });
        state.current_session = Some(session);
        state.active_workspace = Some(workspace);
        state
    }

    #[test]
    fn split_button_keeps_primary_and_picker_commands_separate() {
        let state = state_with_workspace();
        assert!(matches!(
            OpenWithMenu::command_for_widget(&state, OPEN_WITH_PRIMARY_ID),
            Some(AppCommand::OpenWorkspaceExternally {
                application: ExternalApplication::Finder,
                ..
            })
        ));
        assert_eq!(
            OpenWithMenu::command_for_widget(&state, OPEN_WITH_DROPDOWN_ID),
            Some(AppCommand::ToggleOpenWithMenu)
        );
    }

    #[test]
    fn menu_has_rows_only_for_scanned_installed_applications() {
        let mut state = state_with_workspace();
        state.open_with.menu_open = true;
        state.open_with.applications = LoadState::Ready(vec![
            ExternalApplication::VisualStudioCode,
            ExternalApplication::Finder,
        ]);
        let snapshot = crate::WorkspaceSnapshot::build(&state, 1_200.0, 800.0, Insets::ZERO);
        let trigger = OpenWithMenu::split_layout(
            crate::ThreadHeader::layout(snapshot.layout.top_bar, &state)
                .open_with
                .unwrap()
                .rect,
        );
        let menu =
            OpenWithMenu::menu_layout(trigger.rect, snapshot.layout.viewport, &state).unwrap();
        assert_eq!(menu.items.len(), 2);
        assert_eq!(
            menu.items[0].application,
            ExternalApplication::VisualStudioCode
        );
        assert_eq!(menu.items[1].application, ExternalApplication::Finder);
    }

    #[test]
    fn menu_selection_tracks_the_effective_primary_application() {
        let mut state = state_with_workspace();
        state.open_with.menu_open = true;
        state.open_with.applications = LoadState::Ready(vec![
            ExternalApplication::VisualStudioCode,
            ExternalApplication::Finder,
        ]);
        let selected = |state: &zode_app_model::ZodeAppState| state.open_with.primary_application();

        assert_eq!(selected(&state), ExternalApplication::Finder);
        state.open_with.preferred = Some(ExternalApplication::VisualStudioCode);
        assert_eq!(selected(&state), ExternalApplication::VisualStudioCode);
    }

    #[test]
    fn header_actions_follow_open_summary_panel_order_at_wide_width() {
        let state = state_with_workspace();
        let layout =
            crate::ThreadHeader::layout(jian_widgets::Rect::xywh(0.0, 0.0, 1_200.0, 46.0), &state);
        let open_with = layout.open_with.expect("open with");
        let summary = layout.environment.expect("pinned summary");
        let panel = layout.panel_picker.expect("auxiliary panel");
        assert!(open_with.rect.max_x() < summary.rect.origin.x);
        assert!(summary.rect.max_x() < panel.rect.origin.x);
        assert!(layout.review.is_none());
    }

    #[test]
    fn header_focus_order_matches_the_visual_action_order() {
        let state = state_with_workspace();
        let snapshot = crate::WorkspaceSnapshot::build(&state, 1_200.0, 800.0, Insets::ZERO);
        let order = |id| snapshot.node(id).unwrap().focus_order.unwrap();

        assert!(order(crate::widgets::HEADER_MORE_ID) < order(OPEN_WITH_PRIMARY_ID));
        assert!(order(OPEN_WITH_PRIMARY_ID) < order(OPEN_WITH_DROPDOWN_ID));
        assert!(order(OPEN_WITH_DROPDOWN_ID) < order(crate::HEADER_ENVIRONMENT_ID));
        assert!(order(crate::HEADER_ENVIRONMENT_ID) < order(crate::PANEL_PICKER_ID));
    }

    #[test]
    fn compact_header_hides_open_with_before_summary_and_panel() {
        let state = state_with_workspace();
        let layout =
            crate::ThreadHeader::layout(jian_widgets::Rect::xywh(0.0, 0.0, 240.0, 46.0), &state);
        assert!(layout.open_with.is_none());
        let summary = layout.environment.expect("pinned summary");
        let panel = layout.panel_picker.expect("auxiliary panel");
        assert!(summary.rect.max_x() < panel.rect.origin.x);
    }

    #[test]
    fn extremely_narrow_header_never_overlaps_action_hit_targets() {
        let state = state_with_workspace();
        let rect = jian_widgets::Rect::xywh(0.0, 0.0, 80.0, 46.0);
        let layout = crate::ThreadHeader::layout(rect, &state);
        assert!(layout.open_with.is_none());
        assert!(layout.environment.is_none());
        let panel = layout.panel_picker.expect("remaining auxiliary panel");
        assert!(panel.rect.origin.x >= rect.origin.x);
        assert!(panel.rect.max_x() <= rect.max_x());
        assert!(panel.rect.size.x > 0.0 && panel.rect.size.y > 0.0);
    }

    #[test]
    fn projectless_tasks_do_not_expose_project_opening_controls() {
        let mut state = state_with_workspace();
        state.projectless_workspace_root = state.active_workspace.clone();
        let layout =
            crate::ThreadHeader::layout(jian_widgets::Rect::xywh(0.0, 0.0, 1_200.0, 46.0), &state);
        assert!(layout.open_with.is_none());
        assert_eq!(
            OpenWithMenu::command_for_widget(&state, OPEN_WITH_PRIMARY_ID),
            None
        );
        assert_eq!(
            OpenWithMenu::command_for_widget(&state, OPEN_WITH_DROPDOWN_ID),
            None
        );
    }
}
