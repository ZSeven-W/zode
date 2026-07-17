use jian_widgets::Rect;
use zode_app_model::LayoutClass;

pub const SIDEBAR_W: f32 = 240.0;
pub const COMPACT_SIDEBAR_W: f32 = 64.0;
pub const TOP_BAR_H: f32 = 46.0;
pub const CONTENT_W: f32 = 736.0;
pub const COMPOSER_H: f32 = 100.0;
pub const COMPOSER_BOTTOM: f32 = 14.0;
pub const CONTENT_GUTTER: f32 = 16.0;
pub const TRANSCRIPT_TOP_GAP: f32 = 24.0;
pub const TRANSCRIPT_COMPOSER_GAP: f32 = 28.0;

/// Insets supplied by the platform host in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
}

/// Geometry helpers missing from the upstream Jian rectangle primitive.
pub trait RectExt {
    fn min_x(self) -> f32;
    fn min_y(self) -> f32;
    fn max_x(self) -> f32;
    fn max_y(self) -> f32;
    fn width(self) -> f32;
    fn height(self) -> f32;
}

impl RectExt for Rect {
    fn min_x(self) -> f32 {
        self.origin.x
    }

    fn min_y(self) -> f32 {
        self.origin.y
    }

    fn max_x(self) -> f32 {
        self.origin.x + self.size.x
    }

    fn max_y(self) -> f32 {
        self.origin.y + self.size.y
    }

    fn width(self) -> f32 {
        self.size.x
    }

    fn height(self) -> f32 {
        self.size.y
    }
}

/// Shared layout snapshot used by painting, hit-testing and accessibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceLayout {
    pub class: LayoutClass,
    pub viewport: Rect,
    pub sidebar: Rect,
    pub top_bar: Rect,
    pub transcript: Rect,
    pub composer: Rect,
    pub context_panel: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceLayoutOptions {
    pub context_panel_width: f32,
}

impl Default for WorkspaceLayoutOptions {
    fn default() -> Self {
        Self {
            context_panel_width: 0.0,
        }
    }
}

impl WorkspaceLayout {
    pub fn compute(width: f32, height: f32, insets: Insets) -> Self {
        Self::compute_with_options(width, height, insets, WorkspaceLayoutOptions::default())
    }

    pub fn compute_with_options(
        width: f32,
        height: f32,
        insets: Insets,
        options: WorkspaceLayoutOptions,
    ) -> Self {
        let width = finite_non_negative(width);
        let height = finite_non_negative(height);
        let insets = Insets {
            top: finite_non_negative(insets.top).min(height),
            right: finite_non_negative(insets.right).min(width),
            bottom: finite_non_negative(insets.bottom).min(height),
            left: finite_non_negative(insets.left).min(width),
        };
        let class = LayoutClass::for_width(width);
        let available_w = (width - insets.left - insets.right).max(0.0);
        let available_h = (height - insets.top - insets.bottom).max(0.0);
        let desired_sidebar = match class {
            LayoutClass::Wide => SIDEBAR_W,
            LayoutClass::Compact => COMPACT_SIDEBAR_W,
            LayoutClass::Phone => 0.0,
        };
        let sidebar_w = desired_sidebar.min(available_w);
        let main_x = insets.left + sidebar_w;
        let main_w = (available_w - sidebar_w).max(0.0);
        let horizontal_gutters = (CONTENT_GUTTER * 2.0).min(main_w);
        let content_w = CONTENT_W.min((main_w - horizontal_gutters).max(0.0));
        let content_x = main_x + (main_w - content_w) / 2.0;

        let top_bar_h = TOP_BAR_H.min(available_h);
        let composer_h = COMPOSER_H
            .min((available_h - top_bar_h - COMPOSER_BOTTOM - TRANSCRIPT_TOP_GAP).max(0.0));
        let composer_y =
            (height - insets.bottom - COMPOSER_BOTTOM - composer_h).max(insets.top + top_bar_h);
        let transcript_y = insets.top + top_bar_h + TRANSCRIPT_TOP_GAP;
        let transcript_bottom = (composer_y - TRANSCRIPT_COMPOSER_GAP).max(transcript_y);
        let requested_context_w = finite_non_negative(options.context_panel_width);
        let context_right = (width - insets.right - CONTENT_GUTTER).max(0.0);
        let context_left = content_x + content_w + CONTENT_GUTTER;
        let available_context_w = (context_right - context_left).max(0.0);
        let context_w = if class == LayoutClass::Wide {
            requested_context_w.min(available_context_w)
        } else {
            0.0
        };
        let context_x = (context_right - context_w).max(context_left.min(context_right));
        let context_y = insets.top + top_bar_h + CONTENT_GUTTER;
        let context_h = (height - insets.bottom - context_y - CONTENT_GUTTER).max(0.0);

        Self {
            class,
            viewport: Rect::xywh(0.0, 0.0, width, height),
            sidebar: Rect::xywh(insets.left, insets.top, sidebar_w, available_h),
            top_bar: Rect::xywh(main_x, insets.top, main_w, top_bar_h),
            transcript: Rect::xywh(
                content_x,
                transcript_y,
                content_w,
                transcript_bottom - transcript_y,
            ),
            composer: Rect::xywh(content_x, composer_y, content_w, composer_h),
            context_panel: Rect::xywh(context_x, context_y, context_w, context_h),
        }
    }
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
