use jian_widgets::Rect;
use zode_app_model::{LayoutClass, SecondaryPane, ShellRoute};

pub const SIDEBAR_W: f32 = 240.0;
pub const COMPACT_SIDEBAR_W: f32 = 64.0;
pub const TOP_BAR_H: f32 = 46.0;
pub const CONTENT_W: f32 = 736.0;
pub const COMPOSER_CONTEXT_H: f32 = 44.0;
pub const COMPOSER_ATTACHMENT_H: f32 = 52.0;
pub const COMPOSER_INPUT_H: f32 = 100.0;
pub const COMPOSER_H: f32 = COMPOSER_CONTEXT_H + COMPOSER_INPUT_H;
pub const COMPOSER_BOTTOM: f32 = 14.0;
pub const CONTENT_GUTTER: f32 = 16.0;
pub const TRANSCRIPT_TOP_GAP: f32 = 24.0;
pub const TRANSCRIPT_COMPOSER_GAP: f32 = 28.0;
pub const SETTINGS_CONTENT_W: f32 = 768.0;
pub const ENVIRONMENT_PANEL_W: f32 = 300.0;
pub const REVIEW_PANEL_W: f32 = 700.0;
pub const SECONDARY_PANE_BREAKPOINT: f32 = 1400.0;
pub const SPLIT_DIVIDER_W: f32 = 1.0;

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
    pub primary_surface: Rect,
    pub transcript: Rect,
    pub composer: Rect,
    pub page_content: Rect,
    pub context_panel: Rect,
    pub divider: Rect,
    pub review_panel: Rect,
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
        Self::compute_internal(
            width,
            height,
            insets,
            ShellRoute::Conversation,
            SecondaryLayout::None,
            false,
        )
    }

    pub fn compute_with_options(
        width: f32,
        height: f32,
        insets: Insets,
        options: WorkspaceLayoutOptions,
    ) -> Self {
        let context_width = finite_non_negative(options.context_panel_width);
        let secondary = if context_width > 0.0 {
            SecondaryLayout::Environment(context_width)
        } else {
            SecondaryLayout::None
        };
        Self::compute_internal(
            width,
            height,
            insets,
            ShellRoute::Conversation,
            secondary,
            false,
        )
    }

    /// Computes geometry for a typed desktop route and its optional side pane.
    pub fn compute_presentation(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary_pane: Option<SecondaryPane>,
    ) -> Self {
        let secondary = match secondary_pane {
            Some(SecondaryPane::Environment) => SecondaryLayout::Environment(ENVIRONMENT_PANEL_W),
            Some(SecondaryPane::Review) => SecondaryLayout::Review,
            None => SecondaryLayout::None,
        };
        Self::compute_internal(width, height, insets, route, secondary, false)
    }

    /// Computes the state-dependent composer stack while keeping the rest of
    /// the route geometry identical to `compute_presentation`.
    pub fn compute_presentation_with_attachments(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary_pane: Option<SecondaryPane>,
        has_attachments: bool,
    ) -> Self {
        let secondary = match secondary_pane {
            Some(SecondaryPane::Environment) => SecondaryLayout::Environment(ENVIRONMENT_PANEL_W),
            Some(SecondaryPane::Review) => SecondaryLayout::Review,
            None => SecondaryLayout::None,
        };
        Self::compute_internal(width, height, insets, route, secondary, has_attachments)
    }

    fn compute_internal(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary: SecondaryLayout,
        has_attachments: bool,
    ) -> Self {
        let width = finite_non_negative(width);
        let height = finite_non_negative(height);
        let insets = normalized_insets(width, height, insets);
        let class = LayoutClass::for_width(width);
        let available_w = (width - insets.left - insets.right).max(0.0);
        let available_h = (height - insets.top - insets.bottom).max(0.0);
        let safe_right = width - insets.right;
        let safe_bottom = height - insets.bottom;
        let desired_sidebar = match class {
            LayoutClass::Wide => SIDEBAR_W,
            LayoutClass::Compact => COMPACT_SIDEBAR_W,
            LayoutClass::Phone => 0.0,
        };
        let sidebar_w = desired_sidebar.min(available_w);
        let main_x = insets.left + sidebar_w;
        let main_w = (available_w - sidebar_w).max(0.0);

        let secondary_visible = width >= SECONDARY_PANE_BREAKPOINT && main_w > 0.0;
        let mut primary_right = safe_right;
        let mut divider = empty_rect(safe_right, insets.top);
        let mut review_panel = empty_rect(safe_right, insets.top);
        if secondary_visible && secondary == SecondaryLayout::Review {
            let review_w = REVIEW_PANEL_W.min(main_w * 0.45);
            let review_x = safe_right - review_w;
            let divider_w = SPLIT_DIVIDER_W.min((review_x - main_x).max(0.0));
            let divider_x = review_x - divider_w;
            primary_right = divider_x;
            divider = Rect::xywh(divider_x, insets.top, divider_w, available_h);
            review_panel = Rect::xywh(review_x, insets.top, review_w, available_h);
        }
        let primary_w = (primary_right - main_x).max(0.0);
        let primary_surface = Rect::xywh(main_x, insets.top, primary_w, available_h);

        let environment_frame = if secondary_visible {
            match secondary {
                SecondaryLayout::Environment(requested_w) => {
                    let panel_right = (safe_right - CONTENT_GUTTER.min(main_w)).max(main_x);
                    let panel_w = requested_w.min((panel_right - main_x).max(0.0));
                    Some((panel_right - panel_w, panel_w))
                }
                SecondaryLayout::None | SecondaryLayout::Review => None,
            }
        } else {
            None
        };
        let content_right = environment_frame
            .map(|(panel_x, _)| panel_x - CONTENT_GUTTER.min((panel_x - main_x).max(0.0)))
            .unwrap_or(primary_right);
        let content_region_w = (content_right - main_x).max(0.0);
        let content_gutters = (CONTENT_GUTTER * 2.0).min(content_region_w);
        let content_w = CONTENT_W.min((content_region_w - content_gutters).max(0.0));
        let centered_content_x = main_x + (primary_w - content_w) / 2.0;
        let content_x = centered_content_x.min((content_right - content_w).max(main_x));

        let top_bar_h = TOP_BAR_H.min(available_h);
        let top_bar = Rect::xywh(main_x, insets.top, primary_w, top_bar_h);
        let desired_composer_h = COMPOSER_H
            + if has_attachments {
                COMPOSER_ATTACHMENT_H
            } else {
                0.0
            };
        let composer_h = desired_composer_h
            .min((available_h - top_bar_h - COMPOSER_BOTTOM - TRANSCRIPT_TOP_GAP).max(0.0));
        let composer_bottom_gap = COMPOSER_BOTTOM.min((safe_bottom - top_bar.max_y()).max(0.0));
        let composer_y = (safe_bottom - composer_bottom_gap - composer_h).max(top_bar.max_y());
        let transcript_y = (top_bar.max_y() + TRANSCRIPT_TOP_GAP).min(composer_y);
        let transcript_bottom = (composer_y - TRANSCRIPT_COMPOSER_GAP).max(transcript_y);

        let page_target_w = match route {
            ShellRoute::Settings(_) => SETTINGS_CONTENT_W,
            ShellRoute::Conversation
            | ShellRoute::Terminal
            | ShellRoute::Integrations(_)
            | ShellRoute::ComingSoon(_) => CONTENT_W,
        };
        let page_gutters = (CONTENT_GUTTER * 2.0).min(primary_w);
        let page_w = page_target_w.min((primary_w - page_gutters).max(0.0));
        let page_x = main_x + (primary_w - page_w) / 2.0;
        let page_y = (top_bar.max_y() + TRANSCRIPT_TOP_GAP).min(safe_bottom);
        let page_content = Rect::xywh(page_x, page_y, page_w, (safe_bottom - page_y).max(0.0));

        let mut context_panel = empty_rect(safe_right, insets.top);
        if let Some((context_x, context_w)) = environment_frame {
            let context_y = (top_bar.max_y() + CONTENT_GUTTER).min(safe_bottom);
            let context_bottom_gap = CONTENT_GUTTER.min((safe_bottom - context_y).max(0.0));
            let context_h = (safe_bottom - context_bottom_gap - context_y).max(0.0);
            context_panel = Rect::xywh(context_x, context_y, context_w, context_h);
        }

        Self {
            class,
            viewport: Rect::xywh(0.0, 0.0, width, height),
            sidebar: Rect::xywh(insets.left, insets.top, sidebar_w, available_h),
            top_bar,
            primary_surface,
            transcript: Rect::xywh(
                content_x,
                transcript_y,
                content_w,
                transcript_bottom - transcript_y,
            ),
            composer: Rect::xywh(content_x, composer_y, content_w, composer_h),
            page_content,
            context_panel,
            divider,
            review_panel,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SecondaryLayout {
    None,
    Environment(f32),
    Review,
}

fn normalized_insets(width: f32, height: f32, insets: Insets) -> Insets {
    let left = finite_non_negative(insets.left).min(width);
    let right = finite_non_negative(insets.right).min(width - left);
    let top = finite_non_negative(insets.top).min(height);
    let bottom = finite_non_negative(insets.bottom).min(height - top);
    Insets {
        top,
        right,
        bottom,
        left,
    }
}

fn empty_rect(x: f32, y: f32) -> Rect {
    Rect::xywh(x, y, 0.0, 0.0)
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
