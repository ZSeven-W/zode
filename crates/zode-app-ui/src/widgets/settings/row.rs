use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::AppCommand;

use crate::{paint_single_line, RectExt, WidgetId, ZodeTheme};

pub(super) const CONTENT_WIDTH: f32 = 768.0;
pub(super) const CONTENT_TOP: f32 = 70.0;
pub(super) const SECTION_TOP: f32 = 104.0;
pub(super) const GENERAL_ROW_HEIGHT: f32 = 60.0;
pub(super) const APPEARANCE_ROW_HEIGHT: f32 = 44.0;

const TITLE_TOP: f32 = -8.0;
pub(super) const FIRST_SECTION_TOP: f32 = 66.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SettingRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Option<Rect>,
    pub label_rect: Rect,
    pub value_rect: Rect,
    pub label: &'static str,
    pub value: String,
    pub enabled: bool,
    pub command: Option<AppCommand>,
}

pub(super) fn content_rect(primary_surface: Rect) -> Rect {
    let horizontal_gutter = 32.0_f32.min(primary_surface.size.x);
    let width = CONTENT_WIDTH.min((primary_surface.size.x - horizontal_gutter).max(0.0));
    let x = primary_surface.origin.x + (primary_surface.size.x - width).max(0.0) / 2.0;
    let y = (primary_surface.origin.y + CONTENT_TOP).min(primary_surface.max_y());
    Rect::xywh(x, y, width, (primary_surface.max_y() - y).max(0.0))
}

pub(super) fn setting_row(
    id: WidgetId,
    rect: Rect,
    viewport: Rect,
    label: &'static str,
    value: impl Into<String>,
    enabled: bool,
    command: Option<AppCommand>,
) -> SettingRowLayout {
    let value_width = (rect.size.x * 0.42).clamp(132.0, 300.0);
    let value_rect = Rect::xywh(
        rect.max_x() - value_width - 18.0,
        rect.origin.y,
        value_width,
        rect.size.y,
    );
    let label_rect = Rect::xywh(
        rect.origin.x + 18.0,
        rect.origin.y,
        (value_rect.origin.x - rect.origin.x - 30.0).max(0.0),
        rect.size.y,
    );
    SettingRowLayout {
        id,
        rect,
        visible_rect: clip_to_viewport(rect, viewport),
        label_rect,
        value_rect,
        label,
        value: value.into(),
        enabled,
        command: enabled.then_some(command).flatten(),
    }
}

pub(super) fn paint_heading(
    painter: &mut dyn Painter,
    content: Rect,
    title: &str,
    section: &str,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        title,
        Rect::xywh(
            content.origin.x,
            content.origin.y + TITLE_TOP - offset,
            content.size.x,
            40.0,
        ),
        24.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        section,
        Rect::xywh(
            content.origin.x,
            content.origin.y + FIRST_SECTION_TOP - offset,
            content.size.x,
            24.0,
        ),
        13.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

pub(super) fn paint_section_label(
    painter: &mut dyn Painter,
    rect: Rect,
    label: &str,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        label,
        rect,
        13.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

pub(super) fn paint_card(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
}

pub(super) fn paint_divider(
    painter: &mut dyn Painter,
    card: Rect,
    y_offset: f32,
    theme: &ZodeTheme,
) {
    let y = card.origin.y + y_offset;
    painter.stroke_line(
        Point2D::new(card.origin.x + 16.0, y),
        Point2D::new(card.max_x() - 16.0, y),
        theme.tokens.border,
        1.0,
    );
}

pub(super) fn paint_setting_row(
    painter: &mut dyn Painter,
    layout: &SettingRowLayout,
    theme: &ZodeTheme,
) {
    let foreground = if layout.enabled {
        theme.tokens.foreground
    } else {
        theme.tokens.muted_foreground.with_alpha(0.72)
    };
    paint_single_line(
        painter,
        layout.label,
        layout.label_rect,
        13.0,
        if layout.enabled { 500 } else { 450 },
        foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        &layout.value,
        layout.value_rect,
        12.0,
        450,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
}

pub(super) fn paint_placeholder(
    painter: &mut dyn Painter,
    content: Rect,
    title: &str,
    message: &str,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, title, "即将支持", 0.0, theme);
    let card = Rect::xywh(
        content.origin.x,
        content.origin.y + SECTION_TOP,
        content.size.x,
        136.0,
    );
    paint_card(painter, card, theme);
    paint_single_line(
        painter,
        message,
        Rect::xywh(
            card.origin.x + 18.0,
            card.origin.y + 24.0,
            (card.size.x - 36.0).max(0.0),
            32.0,
        ),
        14.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        "接入真实本地状态后，这里会提供对应设置。",
        Rect::xywh(
            card.origin.x + 18.0,
            card.origin.y + 62.0,
            (card.size.x - 36.0).max(0.0),
            28.0,
        ),
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
}

pub(super) fn clip_to_viewport(rect: Rect, viewport: Rect) -> Option<Rect> {
    let left = rect.origin.x.max(viewport.origin.x);
    let top = rect.origin.y.max(viewport.origin.y);
    let right = rect.max_x().min(viewport.max_x());
    let bottom = rect.max_y().min(viewport.max_y());
    (right > left && bottom > top).then(|| Rect::xywh(left, top, right - left, bottom - top))
}
