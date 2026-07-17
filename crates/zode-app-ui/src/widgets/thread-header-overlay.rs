use jian_core::text_input::TextInputState;
use jian_widgets::{
    components::{input::Input, popover::Popover},
    HorizontalAlign, Painter, Rect,
};
use zode_app_model::ZodeAppState;

use super::thread_header::{
    ThreadCopyMenuLayout, ThreadMenuActionLayout, ThreadMenuLayout, ThreadRenameLayout,
    HEADER_RENAME_INPUT_ID,
};
use crate::{
    paint_elevated_surface, paint_single_line, MenuRow, RectExt, SemanticIcon, WidgetId, ZodeTheme,
};

pub(super) fn paint_task_menu(
    painter: &mut dyn Painter,
    menu: &ThreadMenuLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_popover_surface(painter, menu.rect, theme);
    let pinned = state
        .current_session
        .as_ref()
        .is_some_and(|session| state.pinned_sessions.contains(session));
    let rows = [
        (
            menu.pin,
            SemanticIcon::Pin,
            if pinned {
                "取消置顶"
            } else {
                "置顶任务"
            },
            None,
        ),
        (menu.rename, SemanticIcon::Edit, "重命名任务", None),
        (menu.archive, SemanticIcon::Archive, "归档任务", None),
        (
            menu.side_task,
            SemanticIcon::Worktree,
            "打开侧边任务",
            Some("即将支持"),
        ),
        (menu.copy, SemanticIcon::FileText, "复制", Some("›")),
        (
            menu.continue_in,
            SemanticIcon::ExternalOpen,
            "在…中继续",
            Some("即将支持"),
        ),
        (
            menu.schedule,
            SemanticIcon::Scheduled,
            "添加计划任务…",
            Some("即将支持"),
        ),
        (
            menu.new_window,
            SemanticIcon::ExternalOpen,
            "在新窗口中打开",
            None,
        ),
    ];
    for (action, icon, label, trailing) in rows {
        paint_menu_action(
            painter, action, icon, label, trailing, focused, hovered, theme,
        );
    }
    painter.fill_rect(menu.separator_one, theme.tokens.border);
    painter.fill_rect(menu.separator_two, theme.tokens.border);
    if let Some(copy) = menu.copy_menu {
        paint_copy_menu(painter, &copy, focused, hovered, theme);
    }
}

pub(super) fn paint_rename_dialog(
    painter: &mut dyn Painter,
    layout: &ThreadRenameLayout,
    rename_input: &TextInputState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_popover_surface(painter, layout.rect, theme);
    paint_single_line(
        painter,
        "重命名任务",
        Rect::xywh(
            layout.rect.origin.x + 16.0,
            layout.rect.origin.y + 8.0,
            layout.rect.size.x - 32.0,
            26.0,
        ),
        14.0,
        600,
        theme.tokens.popover_foreground,
        HorizontalAlign::Start,
    );
    Input {
        state: rename_input,
        placeholder: "输入任务名称",
        focused: focused == Some(HEADER_RENAME_INPUT_ID),
        font_size: 13.0,
        now_ms: 0,
        icon_d: None,
    }
    .paint(painter, layout.input, &theme.tokens);
    paint_text_button(
        painter,
        layout.cancel,
        "取消",
        focused,
        hovered,
        false,
        theme,
    );
    paint_text_button(painter, layout.save, "保存", focused, hovered, true, theme);
}

fn paint_copy_menu(
    painter: &mut dyn Painter,
    menu: &ThreadCopyMenuLayout,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_popover_surface(painter, menu.rect, theme);
    for (action, label) in [
        (menu.title, "复制任务标题"),
        (menu.details, "复制任务信息"),
        (menu.session_id, "复制任务 ID"),
    ] {
        paint_menu_action(
            painter,
            action,
            SemanticIcon::FileText,
            label,
            None,
            focused,
            hovered,
            theme,
        );
    }
}

fn paint_popover_surface(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    // `Popover::paint` fills/strokes at its own fixed 6.0 radius
    // (`jian_widgets::components::popover`, which equals `tokens.radius`
    // here) - the shadow now matches that instead of the previous ad hoc
    // 10.0.
    paint_elevated_surface(painter, rect, theme.tokens.radius, theme);
    Popover.paint(painter, rect, &theme.tokens);
}

#[allow(clippy::too_many_arguments)]
fn paint_menu_action(
    painter: &mut dyn Painter,
    action: ThreadMenuActionLayout,
    icon: SemanticIcon,
    label: &str,
    trailing: Option<&str>,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let active = action.enabled && (hovered == Some(action.id) || focused == Some(action.id));
    MenuRow::paint_background(painter, action.rect, active, &theme.tokens);
    let foreground = if action.enabled {
        theme.tokens.popover_foreground
    } else {
        theme.tokens.muted_foreground.with_alpha(0.62)
    };
    let icon_rect = Rect::xywh(
        action.rect.origin.x + 10.0,
        action.rect.origin.y + (action.rect.size.y - 16.0) / 2.0,
        16.0,
        16.0,
    );
    painter.stroke_svg_path(
        icon.path(),
        icon_rect.origin,
        icon_rect.size.x,
        foreground,
        icon.stroke_width(),
    );
    let trailing_width = trailing.map_or(0.0, |value| if value == "›" { 18.0 } else { 64.0 });
    paint_single_line(
        painter,
        label,
        Rect::xywh(
            icon_rect.max_x() + 9.0,
            action.rect.origin.y,
            (action.rect.max_x() - icon_rect.max_x() - 15.0 - trailing_width).max(0.0),
            action.rect.size.y,
        ),
        13.0,
        400,
        foreground,
        HorizontalAlign::Start,
    );
    if let Some(trailing) = trailing {
        paint_single_line(
            painter,
            trailing,
            Rect::xywh(
                action.rect.max_x() - trailing_width - 6.0,
                action.rect.origin.y,
                trailing_width,
                action.rect.size.y,
            ),
            if trailing == "›" { 16.0 } else { 10.0 },
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_text_button(
    painter: &mut dyn Painter,
    action: ThreadMenuActionLayout,
    label: &str,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    primary: bool,
    theme: &ZodeTheme,
) {
    let enabled = action.enabled;
    let background = if primary && enabled {
        theme.tokens.foreground
    } else if (hovered == Some(action.id) || focused == Some(action.id)) && enabled {
        theme.tokens.muted
    } else {
        theme.tokens.card
    };
    painter.fill_round_rect(action.rect, 8.0, background);
    painter.stroke_round_rect(action.rect, 8.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        label,
        action.rect,
        12.0,
        500,
        if !enabled {
            theme.tokens.muted_foreground
        } else if primary {
            theme.tokens.background
        } else {
            theme.tokens.foreground
        },
        HorizontalAlign::Center,
    );
}
