use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::ComingSoonFeature;

use crate::ZodeTheme;

pub struct ComingSoonPage;

impl ComingSoonPage {
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        feature: ComingSoonFeature,
        theme: &ZodeTheme,
    ) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let title = feature_label(feature);
        let message = format!("{title}即将支持");
        let center_y = rect.origin.y + rect.size.y * 0.42;
        painter.save();
        painter.clip_rect(rect);
        draw_centered(
            painter,
            title,
            rect,
            center_y,
            26.0,
            600,
            theme.tokens.foreground,
        );
        draw_centered(
            painter,
            &message,
            rect,
            center_y + 34.0,
            14.0,
            400,
            theme.tokens.muted_foreground,
        );
        painter.restore();
    }
}

const fn feature_label(feature: ComingSoonFeature) -> &'static str {
    match feature {
        ComingSoonFeature::ScheduledTasks => "已安排",
        ComingSoonFeature::Sites => "站点",
        ComingSoonFeature::PullRequests => "拉取请求",
        ComingSoonFeature::Chats => "聊天",
        ComingSoonFeature::Help => "帮助",
    }
}

fn draw_centered(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    baseline_y: f32,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let width = painter.measure_text_weighted(text, size, weight);
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(
        &layout,
        Point2D::new(
            rect.origin.x + (rect.size.x - width).max(0.0) / 2.0,
            baseline_y,
        ),
    );
}
