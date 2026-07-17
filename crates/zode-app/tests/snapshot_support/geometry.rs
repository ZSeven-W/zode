use jian_widgets::Rect;
use zode_app_ui::{RectExt, WorkspaceLayout, WorkspaceSnapshot};

const MAX_GEOMETRY_DRIFT_PX: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRect {
    Viewport,
    Sidebar,
    TopBar,
    PrimarySurface,
    Transcript,
    Composer,
    PageContent,
    ContextPanel,
    Divider,
    ReviewPanel,
}

impl LayoutRect {
    fn resolve(self, layout: &WorkspaceLayout) -> Rect {
        match self {
            Self::Viewport => layout.viewport,
            Self::Sidebar => layout.sidebar,
            Self::TopBar => layout.top_bar,
            Self::PrimarySurface => layout.primary_surface,
            Self::Transcript => layout.transcript,
            Self::Composer => layout.composer,
            Self::PageContent => layout.page_content,
            Self::ContextPanel => layout.context_panel,
            Self::Divider => layout.divider,
            Self::ReviewPanel => layout.review_panel,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Viewport => "viewport",
            Self::Sidebar => "sidebar",
            Self::TopBar => "top_bar",
            Self::PrimarySurface => "primary_surface",
            Self::Transcript => "transcript",
            Self::Composer => "composer",
            Self::PageContent => "page_content",
            Self::ContextPanel => "context_panel",
            Self::Divider => "divider",
            Self::ReviewPanel => "review_panel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometryExpectation {
    rect: LayoutRect,
    xywh: [f32; 4],
}

impl GeometryExpectation {
    pub const fn new(rect: LayoutRect, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect,
            xywh: [x, y, width, height],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GeometryDrift {
    rect: LayoutRect,
    component: &'static str,
    expected: f32,
    actual: f32,
    physical_delta: f32,
}

pub fn assert_snapshot_geometry(
    case_name: &str,
    snapshot: &WorkspaceSnapshot,
    physical_scale: f32,
    expectations: &[GeometryExpectation],
) {
    assert!(
        physical_scale.is_finite() && physical_scale > 0.0,
        "{case_name} geometry scale must be finite and positive"
    );
    assert!(
        !expectations.is_empty(),
        "{case_name} must declare fixed geometry expectations"
    );

    let mut worst = None;
    for expectation in expectations {
        let actual = expectation.rect.resolve(&snapshot.layout);
        let components = [
            ("x", expectation.xywh[0], actual.min_x()),
            ("y", expectation.xywh[1], actual.min_y()),
            ("width", expectation.xywh[2], actual.width()),
            ("height", expectation.xywh[3], actual.height()),
        ];
        for (component, expected, actual) in components {
            let drift = GeometryDrift {
                rect: expectation.rect,
                component,
                expected,
                actual,
                physical_delta: (actual - expected).abs() * physical_scale,
            };
            if worst.is_none_or(|candidate: GeometryDrift| {
                drift.physical_delta > candidate.physical_delta
            }) {
                worst = Some(drift);
            }
        }
    }

    let worst = worst.expect("non-empty geometry expectations produce a drift metric");
    assert!(
        worst.physical_delta <= MAX_GEOMETRY_DRIFT_PX,
        "{case_name} geometry drift exceeds {MAX_GEOMETRY_DRIFT_PX}px: {}.{} expected {}, got {} ({} physical px)",
        worst.rect.name(),
        worst.component,
        worst.expected,
        worst.actual,
        worst.physical_delta,
    );
}

#[cfg(test)]
mod tests {
    use zode_app_model::demo_state;
    use zode_app_ui::{Insets, WorkspaceSnapshot};

    use super::*;

    #[test]
    #[should_panic(expected = "geometry drift exceeds 2px")]
    fn geometry_guard_rejects_more_than_two_physical_pixels_of_drift() {
        let snapshot = WorkspaceSnapshot::build(&demo_state(), 1221.0, 992.0, Insets::ZERO);
        let composer = snapshot.layout.composer;
        let drifted = [GeometryExpectation::new(
            LayoutRect::Composer,
            composer.min_x() + 2.01,
            composer.min_y(),
            composer.width(),
            composer.height(),
        )];

        assert_snapshot_geometry("drifted", &snapshot, 1.0, &drifted);
    }
}
