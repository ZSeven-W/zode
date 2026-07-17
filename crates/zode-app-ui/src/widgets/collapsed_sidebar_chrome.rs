use jian_widgets::{Painter, Point2D, Rect};

use crate::{SemanticIcon, WidgetId, ZodeTheme, NEW_SESSION_ID, SIDEBAR_TOGGLE_ID};

const BUTTON_SIZE: f32 = 24.0;
const BUTTON_Y: f32 = 11.0;
const ICON_SIZE: f32 = 14.0;
const TOGGLE_X: f32 = 88.0;
const BACK_X: f32 = 123.0;
const FORWARD_X: f32 = 157.0;
const NEW_TASK_X: f32 = 191.0;

pub const COLLAPSED_SIDEBAR_CHROME_TRAILING_EDGE: f32 = 224.0;
pub const COLLAPSED_SIDEBAR_BACK_ID: WidgetId = WidgetId(9_106);
pub const COLLAPSED_SIDEBAR_FORWARD_ID: WidgetId = WidgetId(9_107);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollapsedSidebarChromeLayout {
    pub toggle: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub new_task: Rect,
    pub trailing_edge: f32,
}

pub struct CollapsedSidebarChrome;

impl CollapsedSidebarChrome {
    pub fn layout(top_bar: Rect) -> CollapsedSidebarChromeLayout {
        let button = |x| {
            Rect::xywh(
                top_bar.origin.x + x,
                top_bar.origin.y + BUTTON_Y,
                BUTTON_SIZE,
                BUTTON_SIZE,
            )
        };
        CollapsedSidebarChromeLayout {
            toggle: button(TOGGLE_X),
            back: button(BACK_X),
            forward: button(FORWARD_X),
            new_task: button(NEW_TASK_X),
            trailing_edge: (top_bar.origin.x + COLLAPSED_SIDEBAR_CHROME_TRAILING_EDGE)
                .min(top_bar.origin.x + top_bar.size.x.max(0.0)),
        }
    }

    pub fn content_rect(top_bar: Rect) -> Rect {
        let leading = Self::layout(top_bar).trailing_edge;
        Rect::xywh(
            leading,
            top_bar.origin.y,
            (top_bar.origin.x + top_bar.size.x - leading).max(0.0),
            top_bar.size.y.max(0.0),
        )
    }

    pub fn paint(
        painter: &mut dyn Painter,
        top_bar: Rect,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        Self::paint_with_opacity(painter, top_bar, focused, hovered, 1.0, theme);
    }

    pub fn paint_with_opacity(
        painter: &mut dyn Painter,
        top_bar: Rect,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        opacity: f32,
        theme: &ZodeTheme,
    ) {
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        let layout = Self::layout(top_bar);
        paint_button(
            painter,
            layout.toggle,
            SemanticIcon::Sidebar,
            focused == Some(SIDEBAR_TOGGLE_ID) || hovered == Some(SIDEBAR_TOGGLE_ID),
            opacity,
            theme,
        );
        paint_disabled(painter, layout.back, SemanticIcon::Back, opacity, theme);
        paint_disabled(
            painter,
            layout.forward,
            SemanticIcon::Forward,
            opacity,
            theme,
        );
        paint_button(
            painter,
            layout.new_task,
            SemanticIcon::NewTask,
            focused == Some(NEW_SESSION_ID) || hovered == Some(NEW_SESSION_ID),
            opacity,
            theme,
        );
    }
}

fn paint_button(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    active: bool,
    opacity: f32,
    theme: &ZodeTheme,
) {
    if active {
        painter.fill_round_rect(rect, 6.0, multiplied_alpha(theme.tokens.muted, opacity));
    }
    paint_icon(
        painter,
        rect,
        icon,
        multiplied_alpha(theme.tokens.muted_foreground, opacity),
    );
}

fn paint_disabled(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    opacity: f32,
    theme: &ZodeTheme,
) {
    paint_icon(
        painter,
        rect,
        icon,
        theme.tokens.muted_foreground.with_alpha(0.42 * opacity),
    );
}

fn multiplied_alpha(color: jian_widgets::Color, opacity: f32) -> jian_widgets::Color {
    color.with_alpha(color.a * opacity.clamp(0.0, 1.0))
}

fn paint_icon(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    color: jian_widgets::Color,
) {
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - ICON_SIZE) / 2.0,
            rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
        ),
        ICON_SIZE,
        color,
        icon.stroke_width(),
    );
}
