use jian_widgets::Rect;
use zode_app_model::{LayoutClass, SecondaryPane, ShellRoute};

pub const SIDEBAR_W: f32 = 240.0;
pub const PRIMARY_SIDEBAR_DEFAULT_W: f32 = zode_app_model::DEFAULT_PRIMARY_SIDEBAR_WIDTH as f32;
pub const COMPACT_SIDEBAR_W: f32 = 64.0;
pub const PRIMARY_SIDEBAR_MIN_W: f32 = 220.0;
pub const PRIMARY_SIDEBAR_MAX_VIEWPORT_RATIO: f32 = 0.30;
pub const PRIMARY_SIDEBAR_MIN_MAIN_W: f32 = 560.0;
pub const TOP_BAR_H: f32 = 46.0;
/// Centered thread-column max width. Matches the Codex reference's
/// `--thread-content-max-width: 48rem` (768px at the desktop app's 16px
/// root), 32px wider than zode's previous flat `736.0`.
pub const CONTENT_W: f32 = 768.0;
/// Full painted height of the composer context rail. Its lower edge tucks
/// behind the next surface to create the attached-card treatment.
pub const COMPOSER_CONTEXT_H: f32 = 44.0;
/// Visible content and hit-test height above the input card.
pub const COMPOSER_CONTEXT_VISIBLE_H: f32 = 38.0;
/// Painted tail covered by the input card to visually attach both surfaces.
pub const COMPOSER_CONTEXT_OVERLAP: f32 = COMPOSER_CONTEXT_H - COMPOSER_CONTEXT_VISIBLE_H;
/// Horizontal inset that makes the rail read as a tray behind the input card.
pub const COMPOSER_CONTEXT_INSET_X: f32 = 14.0;
pub const COMPOSER_ATTACHMENT_H: f32 = 52.0;
pub const COMPOSER_INPUT_H: f32 = 100.0;
pub const COMPOSER_H: f32 = COMPOSER_CONTEXT_VISIBLE_H + COMPOSER_INPUT_H;
pub const COMPOSER_QUEUE_MAX_VISIBLE: usize = 4;
pub const COMPOSER_QUEUE_ROW_H: f32 = 29.0;
pub const COMPOSER_QUEUE_PAD_Y: f32 = 5.0;
pub const COMPOSER_QUEUE_INSET_X: f32 = 20.0;
pub const COMPOSER_QUEUE_OVERLAP: f32 = 16.0;
pub const COMPOSER_BOTTOM: f32 = 14.0;
pub const CONTENT_GUTTER: f32 = 16.0;
pub const TRANSCRIPT_TOP_GAP: f32 = 24.0;
pub const TRANSCRIPT_COMPOSER_GAP: f32 = 28.0;
pub const SETTINGS_CONTENT_W: f32 = 768.0;
pub const ENVIRONMENT_PANEL_W: f32 = 300.0;
pub const REVIEW_PANEL_W: f32 = 700.0;
pub const SECONDARY_PANE_BREAKPOINT: f32 = 1400.0;
pub const SPLIT_DIVIDER_W: f32 = 1.0;
pub const SECONDARY_SIDEBAR_MIN_W: f32 = 300.0;
pub const SECONDARY_SIDEBAR_MAX_W: f32 = 700.0;
pub const SECONDARY_SIDEBAR_MAX_VIEWPORT_RATIO: f32 = 0.35;
pub const SECONDARY_SIDEBAR_MIN_PRIMARY_W: f32 = 560.0;

/// Responsive presentation used by the pinned summary panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PinnedSummaryMode {
    #[default]
    Hidden,
    Docked,
    Overlay,
}

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
    /// Full-width primary sidebar content translated behind `sidebar` while
    /// a persistent open/close transition is in flight. At rest this is
    /// identical to `sidebar`.
    pub primary_sidebar_content: Rect,
    pub primary_sidebar_divider: Rect,
    pub top_bar: Rect,
    pub primary_surface: Rect,
    pub transcript: Rect,
    pub composer: Rect,
    pub page_content: Rect,
    /// Visible pinned-summary surface. During a docked transition this is a
    /// trailing-edge clip whose width tracks the animated visibility.
    pub context_panel: Rect,
    /// Full-width pinned-summary content translated behind `context_panel`.
    /// Keeping this width stable prevents cards and labels from reflowing
    /// while the panel slides.
    pub pinned_summary_content: Rect,
    pub pinned_summary: PinnedSummaryMode,
    pub divider: Rect,
    /// Visible secondary-sidebar surface used for clipping and hit testing.
    pub review_panel: Rect,
    /// Full-width secondary-sidebar content translated behind `review_panel`.
    pub secondary_sidebar_content: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceLayoutOptions {
    pub context_panel_width: f32,
    pub primary_sidebar: PrimarySidebarLayoutOptions,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrimarySidebarLayoutOptions {
    pub open: bool,
    pub width: f32,
    /// Visible share of the constrained width. Hosts animate this value while
    /// keeping `open` true until a closing transition reaches zero.
    pub visibility: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondarySidebarLayoutOptions {
    pub open: bool,
    pub width: f32,
    pub pinned_summary_overlay: bool,
    /// Visible share of the constrained width used by docked transitions.
    pub visibility: f32,
    /// Visible share of the pinned-summary surface. Overlay summaries translate
    /// from the trailing edge; docked summaries participate in content reflow.
    pub pinned_summary_visibility: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SidebarLayoutOptions {
    pub primary: PrimarySidebarLayoutOptions,
    pub secondary: SecondarySidebarLayoutOptions,
}

impl Default for SecondarySidebarLayoutOptions {
    fn default() -> Self {
        Self {
            open: false,
            width: REVIEW_PANEL_W,
            pinned_summary_overlay: false,
            visibility: 1.0,
            pinned_summary_visibility: 1.0,
        }
    }
}

impl Default for PrimarySidebarLayoutOptions {
    fn default() -> Self {
        Self {
            open: true,
            width: PRIMARY_SIDEBAR_DEFAULT_W,
            visibility: 1.0,
        }
    }
}

impl Default for WorkspaceLayoutOptions {
    fn default() -> Self {
        Self {
            context_panel_width: 0.0,
            primary_sidebar: PrimarySidebarLayoutOptions::default(),
        }
    }
}

impl WorkspaceLayout {
    /// Current painted share of the persistent primary sidebar.
    ///
    /// The full-width content rect stays alive during a transition while the
    /// visible clip changes. Exposing the ratio lets top-bar chrome follow the
    /// same animation instead of switching from the final product boolean.
    pub fn primary_sidebar_visibility(self) -> f32 {
        if self.primary_sidebar_content.size.x > f32::EPSILON {
            finite_unit(self.sidebar.size.x / self.primary_sidebar_content.size.x)
        } else if self.sidebar.size.x > f32::EPSILON {
            1.0
        } else {
            0.0
        }
    }

    pub fn compute(width: f32, height: f32, insets: Insets) -> Self {
        Self::compute_internal(
            width,
            height,
            insets,
            ShellRoute::Conversation,
            SecondaryLayout::None,
            COMPOSER_H,
            SidebarLayoutOptions::default(),
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
            COMPOSER_H,
            SidebarLayoutOptions {
                primary: options.primary_sidebar,
                ..SidebarLayoutOptions::default()
            },
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
        let secondary = secondary_layout(secondary_pane);
        Self::compute_internal(
            width,
            height,
            insets,
            route,
            secondary,
            COMPOSER_H,
            SidebarLayoutOptions {
                secondary: secondary_sidebar_for_pane(secondary_pane),
                ..SidebarLayoutOptions::default()
            },
        )
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
        let secondary = secondary_layout(secondary_pane);
        let composer_height = COMPOSER_H
            + if has_attachments {
                COMPOSER_ATTACHMENT_H
            } else {
                0.0
            };
        Self::compute_internal(
            width,
            height,
            insets,
            route,
            secondary,
            composer_height,
            SidebarLayoutOptions {
                secondary: secondary_sidebar_for_pane(secondary_pane),
                ..SidebarLayoutOptions::default()
            },
        )
    }

    /// Computes the shell around an already measured state-dependent composer.
    /// The composer remains bottom anchored while queue/attachment rows grow upward.
    pub fn compute_presentation_with_composer_height(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary_pane: Option<SecondaryPane>,
        composer_height: f32,
    ) -> Self {
        let secondary = secondary_layout(secondary_pane);
        Self::compute_internal(
            width,
            height,
            insets,
            route,
            secondary,
            composer_height,
            SidebarLayoutOptions {
                secondary: secondary_sidebar_for_pane(secondary_pane),
                ..SidebarLayoutOptions::default()
            },
        )
    }

    pub fn compute_presentation_with_secondary_sidebar(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary_pane: Option<SecondaryPane>,
        composer_height: f32,
        secondary_sidebar: SecondarySidebarLayoutOptions,
    ) -> Self {
        Self::compute_internal(
            width,
            height,
            insets,
            route,
            secondary_layout(secondary_pane),
            composer_height,
            SidebarLayoutOptions {
                secondary: secondary_sidebar,
                ..SidebarLayoutOptions::default()
            },
        )
    }

    pub fn compute_presentation_with_sidebar_options(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary_pane: Option<SecondaryPane>,
        composer_height: f32,
        sidebars: SidebarLayoutOptions,
    ) -> Self {
        Self::compute_internal(
            width,
            height,
            insets,
            route,
            secondary_layout(secondary_pane),
            composer_height,
            sidebars,
        )
    }

    fn compute_internal(
        width: f32,
        height: f32,
        insets: Insets,
        route: ShellRoute,
        secondary: SecondaryLayout,
        desired_composer_h: f32,
        sidebars: SidebarLayoutOptions,
    ) -> Self {
        let SidebarLayoutOptions {
            primary: primary_sidebar,
            secondary: secondary_sidebar,
        } = sidebars;
        let width = finite_non_negative(width);
        let height = finite_non_negative(height);
        let insets = normalized_insets(width, height, insets);
        let class = LayoutClass::for_width(width);
        let available_w = (width - insets.left - insets.right).max(0.0);
        let available_h = (height - insets.top - insets.bottom).max(0.0);
        let safe_right = width - insets.right;
        let safe_bottom = height - insets.bottom;
        let fixed_settings_sidebar = matches!(route, ShellRoute::Settings(_));
        let primary_sidebar_open = fixed_settings_sidebar || primary_sidebar.open;
        let full_sidebar_w = match (class, primary_sidebar_open) {
            (LayoutClass::Wide, true) if fixed_settings_sidebar => SIDEBAR_W,
            (LayoutClass::Wide, true) => {
                constrained_primary_sidebar_width(available_w, primary_sidebar.width)
            }
            (LayoutClass::Compact, true) => COMPACT_SIDEBAR_W,
            (LayoutClass::Wide | LayoutClass::Compact | LayoutClass::Phone, false)
            | (LayoutClass::Phone, true) => 0.0,
        };
        let primary_visibility = if fixed_settings_sidebar {
            1.0
        } else {
            finite_unit(primary_sidebar.visibility)
        };
        let full_sidebar_w = full_sidebar_w.min(available_w);
        let sidebar_w = snap_animated_width(full_sidebar_w, primary_visibility);
        let main_x = insets.left + sidebar_w;
        let main_w = (available_w - sidebar_w).max(0.0);
        let primary_sidebar_divider =
            if class == LayoutClass::Wide && sidebar_w > 0.0 && !fixed_settings_sidebar {
                Rect::xywh(main_x, insets.top, SPLIT_DIVIDER_W, available_h)
            } else {
                empty_rect(main_x, insets.top)
            };

        let secondary_visible = width >= SECONDARY_PANE_BREAKPOINT && main_w > 0.0;
        let mut primary_right = safe_right;
        let mut divider = empty_rect(safe_right, insets.top);
        let mut review_panel = empty_rect(safe_right, insets.top);
        let mut secondary_sidebar_content = empty_rect(safe_right, insets.top);
        if secondary_visible && route == ShellRoute::Conversation && secondary_sidebar.open {
            let full_review_w =
                constrained_secondary_sidebar_width(available_w, main_w, secondary_sidebar.width);
            let review_w =
                snap_animated_width(full_review_w, finite_unit(secondary_sidebar.visibility));
            let review_x = safe_right - review_w;
            let divider_w = SPLIT_DIVIDER_W.min((review_x - main_x).max(0.0));
            let divider_x = review_x - divider_w;
            primary_right = divider_x;
            divider = Rect::xywh(divider_x, insets.top, divider_w, available_h);
            review_panel = Rect::xywh(review_x, insets.top, review_w, available_h);
            secondary_sidebar_content =
                Rect::xywh(review_x, insets.top, full_review_w, available_h);
        }
        let primary_w = (primary_right - main_x).max(0.0);
        let primary_surface = Rect::xywh(main_x, insets.top, primary_w, available_h);

        let pinned_summary = match secondary {
            SecondaryLayout::Environment(_) if secondary_sidebar.pinned_summary_overlay => {
                PinnedSummaryMode::Overlay
            }
            SecondaryLayout::Environment(_) if secondary_visible => PinnedSummaryMode::Docked,
            SecondaryLayout::Environment(_)
                if route == ShellRoute::Conversation && main_w > 0.0 =>
            {
                PinnedSummaryMode::Overlay
            }
            SecondaryLayout::None | SecondaryLayout::Environment(_) | SecondaryLayout::Review => {
                PinnedSummaryMode::Hidden
            }
        };
        let pinned_summary_visibility = finite_unit(secondary_sidebar.pinned_summary_visibility);
        let environment_frame = match (secondary, pinned_summary) {
            (SecondaryLayout::Environment(requested_w), PinnedSummaryMode::Docked) => {
                let panel_right = (primary_right - CONTENT_GUTTER.min(primary_w)).max(main_x);
                let full_panel_w = requested_w.min((panel_right - main_x).max(0.0));
                let visible_panel_w = snap_animated_width(full_panel_w, pinned_summary_visibility);
                let panel_x = panel_right - visible_panel_w;
                Some((panel_x, visible_panel_w, panel_x, full_panel_w))
            }
            (SecondaryLayout::Environment(requested_w), PinnedSummaryMode::Overlay) => {
                let horizontal_gutter = CONTENT_GUTTER.min(primary_w / 2.0);
                let panel_right = (primary_right - horizontal_gutter).max(main_x);
                let panel_w = requested_w.min((primary_w - horizontal_gutter * 2.0).max(0.0));
                let hidden_offset = snap_animated_width(panel_w, 1.0 - pinned_summary_visibility);
                let content_x = panel_right - panel_w + hidden_offset;
                let visible_x = content_x.max(main_x);
                let visible_right = (content_x + panel_w).min(panel_right);
                Some((
                    visible_x,
                    (visible_right - visible_x).max(0.0),
                    content_x,
                    panel_w,
                ))
            }
            _ => None,
        };
        let content_right = if pinned_summary == PinnedSummaryMode::Docked {
            environment_frame
                .map(|(panel_x, _, _, _)| panel_x)
                .unwrap_or(primary_right)
        } else {
            primary_right
        };
        let content_region_w = (content_right - main_x).max(0.0);
        let content_gutters = (CONTENT_GUTTER * 2.0).min(content_region_w);
        let content_w = CONTENT_W.min((content_region_w - content_gutters).max(0.0));
        // A docked summary is part of the wide-screen composition rather than
        // an overlay. Center the transcript/composer in the remaining column
        // between the project rail and the summary card. Using `primary_w`
        // here would keep the content centered under the whole window and then
        // merely clamp it, which visibly pushes the conversation to the right.
        //
        // Rounded rather than left as a raw fraction: `main_x` and the animated
        // sidebar/panel widths above are already snapped to whole pixels, but
        // `content_region_w - content_w` can still be odd, leaving a `.5`
        // remainder here. Snapping it keeps every main-content paint origin on
        // an integral device-pixel phase during animation (see `snap_animated_width`).
        let content_x = main_x + ((content_region_w - content_w) / 2.0).round();

        let top_bar_h = TOP_BAR_H.min(available_h);
        let top_bar = Rect::xywh(main_x, insets.top, primary_w, top_bar_h);
        let composer_h = finite_non_negative(desired_composer_h)
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
        let mut pinned_summary_content = empty_rect(safe_right, insets.top);
        if let Some((context_x, context_w, content_x, content_w)) = environment_frame {
            let context_y = (top_bar.max_y() + CONTENT_GUTTER).min(safe_bottom);
            let context_bottom_gap = CONTENT_GUTTER.min((safe_bottom - context_y).max(0.0));
            let context_h = (safe_bottom - context_bottom_gap - context_y).max(0.0);
            context_panel = Rect::xywh(context_x, context_y, context_w, context_h);
            pinned_summary_content = Rect::xywh(content_x, context_y, content_w, context_h);
        }

        Self {
            class,
            viewport: Rect::xywh(0.0, 0.0, width, height),
            sidebar: Rect::xywh(insets.left, insets.top, sidebar_w, available_h),
            primary_sidebar_content: Rect::xywh(
                insets.left + sidebar_w - full_sidebar_w,
                insets.top,
                full_sidebar_w,
                available_h,
            ),
            primary_sidebar_divider,
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
            pinned_summary_content,
            pinned_summary,
            divider,
            review_panel,
            secondary_sidebar_content,
        }
    }
}

pub fn constrained_primary_sidebar_width(available_width: f32, requested_width: f32) -> f32 {
    let available_width = finite_non_negative(available_width);
    let maximum = (available_width * PRIMARY_SIDEBAR_MAX_VIEWPORT_RATIO)
        .min((available_width - PRIMARY_SIDEBAR_MIN_MAIN_W).max(0.0));
    if maximum <= 0.0 {
        return 0.0;
    }
    let minimum = PRIMARY_SIDEBAR_MIN_W.min(maximum);
    finite_non_negative(requested_width).clamp(minimum, maximum)
}

pub fn constrained_secondary_sidebar_width(
    available_width: f32,
    main_width: f32,
    requested_width: f32,
) -> f32 {
    let available_width = finite_non_negative(available_width);
    let main_width = finite_non_negative(main_width);
    let maximum = SECONDARY_SIDEBAR_MAX_W
        .min(available_width * SECONDARY_SIDEBAR_MAX_VIEWPORT_RATIO)
        .min(main_width * 0.45)
        .min((main_width - SECONDARY_SIDEBAR_MIN_PRIMARY_W - SPLIT_DIVIDER_W).max(0.0));
    if maximum <= 0.0 {
        return 0.0;
    }
    let minimum = SECONDARY_SIDEBAR_MIN_W.min(maximum);
    finite_non_negative(requested_width).clamp(minimum, maximum)
}

pub fn composer_queue_reserved_height(item_count: usize) -> f32 {
    let visible = item_count.min(COMPOSER_QUEUE_MAX_VISIBLE);
    if visible == 0 {
        0.0
    } else {
        COMPOSER_QUEUE_PAD_Y * 2.0 + COMPOSER_QUEUE_ROW_H * visible as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SecondaryLayout {
    None,
    Environment(f32),
    Review,
}

const fn secondary_layout(pane: Option<SecondaryPane>) -> SecondaryLayout {
    match pane {
        Some(SecondaryPane::Environment) => SecondaryLayout::Environment(ENVIRONMENT_PANEL_W),
        Some(
            SecondaryPane::Review
            | SecondaryPane::DocumentPreview
            | SecondaryPane::Terminal
            | SecondaryPane::Browser
            | SecondaryPane::Files
            | SecondaryPane::SideTask
            | SecondaryPane::Subagents,
        ) => SecondaryLayout::Review,
        None => SecondaryLayout::None,
    }
}

const fn secondary_sidebar_for_pane(pane: Option<SecondaryPane>) -> SecondarySidebarLayoutOptions {
    SecondarySidebarLayoutOptions {
        open: matches!(
            pane,
            Some(
                SecondaryPane::Review
                    | SecondaryPane::DocumentPreview
                    | SecondaryPane::Terminal
                    | SecondaryPane::Browser
                    | SecondaryPane::Files
                    | SecondaryPane::SideTask
                    | SecondaryPane::Subagents
            )
        ),
        width: REVIEW_PANEL_W,
        pinned_summary_overlay: false,
        visibility: 1.0,
        pinned_summary_visibility: 1.0,
    }
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

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Snaps an animated width (a full extent scaled by an open/close transition
/// ratio in `[0.0, 1.0]`) to a whole logical pixel.
///
/// macOS scale factors are integers, so a whole logical pixel is a whole
/// device pixel too: every downstream paint origin derived from this value
/// keeps a stable device-pixel phase across frames, which keeps the text
/// picture cache (`zode-app`'s `text_picture_cache.rs`, keyed in part on that
/// phase) hitting instead of re-shaping text every animation frame.
///
/// The `0.0` and `1.0` endpoints intentionally bypass rounding and return the
/// input `full_width` verbatim (which may itself be fractional, e.g. a
/// resized sidebar clamped to a viewport-ratio bound) - the fully closed and
/// fully open steady states must match pre-snap geometry exactly so nothing
/// shifts once a transition settles.
fn snap_animated_width(full_width: f32, visibility: f32) -> f32 {
    if visibility <= 0.0 {
        0.0
    } else if visibility >= 1.0 {
        full_width
    } else {
        (full_width * visibility).round()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_animated_width_preserves_exact_closed_and_open_endpoints() {
        assert_eq!(snap_animated_width(293.0, 0.0), 0.0);
        assert_eq!(snap_animated_width(293.0, 1.0), 293.0);
        // A fractional full width (e.g. a viewport-ratio-clamped resize) must
        // still come back unrounded at the fully open endpoint.
        assert_eq!(snap_animated_width(293.3, 1.0), 293.3);
        // Mid-transition, the scaled value snaps to the nearest whole pixel.
        assert_eq!(snap_animated_width(293.0, 0.5), 147.0);
    }

    #[test]
    fn mid_animation_primary_sidebar_snaps_main_content_to_whole_pixels() {
        let layout = WorkspaceLayout::compute_with_options(
            1_800.0,
            1_080.0,
            Insets::ZERO,
            WorkspaceLayoutOptions {
                primary_sidebar: PrimarySidebarLayoutOptions {
                    open: true,
                    width: PRIMARY_SIDEBAR_DEFAULT_W,
                    visibility: 0.37,
                },
                ..WorkspaceLayoutOptions::default()
            },
        );

        for x in [
            layout.sidebar.min_x(),
            layout.sidebar.max_x(),
            layout.primary_surface.min_x(),
            layout.transcript.min_x(),
            layout.composer.min_x(),
        ] {
            assert_eq!(x.fract(), 0.0, "expected a whole logical pixel, got {x}");
        }
    }
}
