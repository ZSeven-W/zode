use thiserror::Error;
use winit::event_loop::ActiveEventLoop;

use crate::{
    window_bootstrap::StartupWindowPlacement,
    window_state::{restore_window_geometry, MonitorWorkArea, WindowGeometry},
};

/// A recoverable failure to obtain the desktop area where normal windows may live.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkAreaError {
    #[error("work-area discovery is unsupported on {platform}: {reason}")]
    Unsupported {
        platform: &'static str,
        reason: &'static str,
    },
    #[error("work-area discovery must run on the platform UI thread")]
    OffMainThread,
    #[error("work-area discovery failed: {0}")]
    QueryFailed(&'static str),
    #[error("the platform returned invalid work-area geometry: {0}")]
    InvalidGeometry(&'static str),
}

/// Supplies the usable bounds of each monitor in winit physical coordinates.
pub trait WorkAreaProvider {
    fn work_areas(&self) -> Result<Vec<MonitorWorkArea>, WorkAreaError>;
}

#[derive(Debug, Clone)]
pub struct PlatformWorkAreaProvider {
    context: crate::platform_work_area::QueryContext,
}

impl PlatformWorkAreaProvider {
    pub fn from_event_loop(event_loop: &ActiveEventLoop) -> Self {
        Self {
            context: crate::platform_work_area::context_from_event_loop(event_loop),
        }
    }
}

impl WorkAreaProvider for PlatformWorkAreaProvider {
    fn work_areas(&self) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        crate::platform_work_area::query_work_areas(&self.context)
    }
}

/// Restores persisted bounds only when genuine platform work areas are known.
///
/// A caller should treat an error as a signal to let the window manager choose
/// the initial position. Full monitor bounds are not a safe substitute because
/// they include reserved system UI such as menu bars, docks, and taskbars.
pub fn restored_geometry_for_startup(
    saved: Option<WindowGeometry>,
    provider: &impl WorkAreaProvider,
) -> Result<Option<WindowGeometry>, WorkAreaError> {
    let Some(saved) = saved else {
        return Ok(None);
    };
    let work_areas = provider.work_areas()?;
    if work_areas.is_empty() {
        return Err(WorkAreaError::QueryFailed(
            "the platform returned no usable monitor work areas",
        ));
    }
    Ok(Some(restore_window_geometry(Some(saved), &work_areas)))
}

/// Resolves startup placement while retaining size and maximize state when
/// platform work-area discovery cannot safely validate a persisted position.
pub fn resolve_startup_placement(
    saved: Option<WindowGeometry>,
    provider: &impl WorkAreaProvider,
) -> (StartupWindowPlacement, Option<WorkAreaError>) {
    match restored_geometry_for_startup(saved, provider) {
        Ok(Some(geometry)) => (StartupWindowPlacement::Positioned(geometry), None),
        Ok(None) => (StartupWindowPlacement::Default, None),
        Err(error) => (
            saved.map_or(
                StartupWindowPlacement::Default,
                StartupWindowPlacement::Unpositioned,
            ),
            Some(error),
        ),
    }
}
