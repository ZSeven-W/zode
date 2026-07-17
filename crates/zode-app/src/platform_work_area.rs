use winit::event_loop::ActiveEventLoop;

use crate::{window_state::MonitorWorkArea, work_area::WorkAreaError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub(crate) struct QueryContext {
    monitors: Vec<MacMonitor>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct MacMonitor {
    native_id: u32,
    bounds: PhysicalRect,
    scale: f64,
}

#[cfg(target_os = "macos")]
pub(crate) fn context_from_event_loop(event_loop: &ActiveEventLoop) -> QueryContext {
    use winit::platform::macos::MonitorHandleExtMacOS;

    let monitors = event_loop
        .available_monitors()
        .map(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            MacMonitor {
                native_id: monitor.native_id(),
                bounds: PhysicalRect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
                scale: monitor.scale_factor(),
            }
        })
        .collect();
    QueryContext { monitors }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub(crate) struct QueryContext {
    monitors: Vec<winit::monitor::MonitorHandle>,
}

#[cfg(target_os = "windows")]
pub(crate) fn context_from_event_loop(event_loop: &ActiveEventLoop) -> QueryContext {
    QueryContext {
        monitors: event_loop.available_monitors().collect(),
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxBackend {
    X11,
    Wayland,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct QueryContext {
    backend: LinuxBackend,
}

#[cfg(target_os = "linux")]
pub(crate) fn context_from_event_loop(event_loop: &ActiveEventLoop) -> QueryContext {
    use winit::platform::{wayland::ActiveEventLoopExtWayland, x11::ActiveEventLoopExtX11};

    let backend = if event_loop.is_wayland() {
        LinuxBackend::Wayland
    } else {
        debug_assert!(event_loop.is_x11());
        LinuxBackend::X11
    };
    QueryContext { backend }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct QueryContext;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) fn context_from_event_loop(_event_loop: &ActiveEventLoop) -> QueryContext {
    QueryContext
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::{runtime::AnyObject, MainThreadMarker};
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSNumber, NSString};

    use super::{MacMonitor, MonitorWorkArea, PhysicalRect, QueryContext, WorkAreaError};

    #[derive(Debug, Clone, Copy)]
    struct LogicalRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[derive(Debug, Clone, Copy)]
    struct AppKitScreen {
        native_id: u32,
        frame: LogicalRect,
        visible: LogicalRect,
    }

    pub(super) fn query_work_areas(
        context: &QueryContext,
    ) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        if context.monitors.is_empty() {
            return Err(WorkAreaError::QueryFailed(
                "winit returned no active macOS monitors",
            ));
        }
        let mtm = MainThreadMarker::new().ok_or(WorkAreaError::OffMainThread)?;
        let appkit_screens = NSScreen::screens(mtm)
            .iter()
            .map(|screen| {
                let frame = screen.frame();
                let visible = screen.visibleFrame();
                Ok(AppKitScreen {
                    native_id: screen_native_id(&screen)?,
                    frame: LogicalRect {
                        x: frame.origin.x,
                        y: frame.origin.y,
                        width: frame.size.width,
                        height: frame.size.height,
                    },
                    visible: LogicalRect {
                        x: visible.origin.x,
                        y: visible.origin.y,
                        width: visible.size.width,
                        height: visible.size.height,
                    },
                })
            })
            .collect::<Result<Vec<_>, WorkAreaError>>()?;

        context
            .monitors
            .iter()
            .map(|monitor| {
                let screen = appkit_screens
                    .iter()
                    .find(|screen| screen.native_id == monitor.native_id)
                    .ok_or(WorkAreaError::QueryFailed(
                        "AppKit and winit monitor identifiers did not match",
                    ))?;
                from_appkit_insets(*monitor, screen.frame, screen.visible)
            })
            .collect()
    }

    fn screen_native_id(screen: &NSScreen) -> Result<u32, WorkAreaError> {
        let description = screen.deviceDescription();
        let key = NSString::from_str("NSScreenNumber");
        let value: objc2::rc::Retained<AnyObject> =
            description
                .objectForKey(&key)
                .ok_or(WorkAreaError::QueryFailed(
                    "AppKit screen has no NSScreenNumber",
                ))?;
        value
            .downcast_ref::<NSNumber>()
            .map(NSNumber::as_u32)
            .ok_or(WorkAreaError::QueryFailed(
                "AppKit NSScreenNumber is not an NSNumber",
            ))
    }

    fn from_appkit_insets(
        monitor: MacMonitor,
        frame: LogicalRect,
        visible: LogicalRect,
    ) -> Result<MonitorWorkArea, WorkAreaError> {
        if !monitor.scale.is_finite() || monitor.scale <= 0.0 {
            return Err(WorkAreaError::InvalidGeometry(
                "screen scale is not positive and finite",
            ));
        }

        // AppKit global origins are logical and do not map linearly into
        // winit's mixed-DPI physical coordinate space. Only convert the four
        // local insets, then anchor them to winit's native-ID-matched bounds.
        let left = physical_inset(visible.x - frame.x, monitor.scale)?;
        let bottom = physical_inset(visible.y - frame.y, monitor.scale)?;
        let right = physical_inset(
            frame.x + frame.width - visible.x - visible.width,
            monitor.scale,
        )?;
        let top = physical_inset(
            frame.y + frame.height - visible.y - visible.height,
            monitor.scale,
        )?;
        apply_insets(monitor.bounds, left, top, right, bottom)
    }

    fn physical_inset(logical: f64, scale: f64) -> Result<u32, WorkAreaError> {
        let physical = (logical * scale).round();
        if !physical.is_finite() || physical < 0.0 || physical > f64::from(u32::MAX) {
            return Err(WorkAreaError::InvalidGeometry(
                "AppKit visible frame lies outside its screen frame",
            ));
        }
        Ok(physical as u32)
    }

    fn apply_insets(
        bounds: PhysicalRect,
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
    ) -> Result<MonitorWorkArea, WorkAreaError> {
        let horizontal = left
            .checked_add(right)
            .ok_or(WorkAreaError::InvalidGeometry("horizontal inset overflow"))?;
        let vertical = top
            .checked_add(bottom)
            .ok_or(WorkAreaError::InvalidGeometry("vertical inset overflow"))?;
        let width = bounds
            .width
            .checked_sub(horizontal)
            .filter(|width| *width > 0)
            .ok_or(WorkAreaError::InvalidGeometry(
                "AppKit horizontal insets consume the monitor",
            ))?;
        let height = bounds
            .height
            .checked_sub(vertical)
            .filter(|height| *height > 0)
            .ok_or(WorkAreaError::InvalidGeometry(
                "AppKit vertical insets consume the monitor",
            ))?;
        let x = i64::from(bounds.x) + i64::from(left);
        let y = i64::from(bounds.y) + i64::from(top);
        Ok(MonitorWorkArea {
            x: i32::try_from(x).map_err(|_| {
                WorkAreaError::InvalidGeometry("work-area x is outside the physical range")
            })?,
            y: i32::try_from(y).map_err(|_| {
                WorkAreaError::InvalidGeometry("work-area y is outside the physical range")
            })?,
            width,
            height,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{from_appkit_insets, LogicalRect, MacMonitor, PhysicalRect};

        #[test]
        fn mixed_scale_negative_origin_uses_local_insets_and_winit_anchor() {
            let area = from_appkit_insets(
                MacMonitor {
                    native_id: 7,
                    bounds: PhysicalRect {
                        x: -2560,
                        y: 200,
                        width: 2560,
                        height: 1600,
                    },
                    scale: 2.0,
                },
                LogicalRect {
                    x: -1280.0,
                    y: 0.0,
                    width: 1280.0,
                    height: 800.0,
                },
                LogicalRect {
                    x: -1270.0,
                    y: 30.0,
                    width: 1260.0,
                    height: 750.0,
                },
            )
            .expect("valid visible frame");

            assert_eq!((area.x, area.y), (-2540, 240));
            assert_eq!((area.width, area.height), (2520, 1500));
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use winit::platform::windows::MonitorHandleExtWindows;

    use super::{MonitorWorkArea, QueryContext, WorkAreaError};

    pub(super) fn query_work_areas(
        context: &QueryContext,
    ) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        if context.monitors.is_empty() {
            return Err(WorkAreaError::QueryFailed(
                "winit returned no active Windows monitors",
            ));
        }
        context
            .monitors
            .iter()
            .map(|monitor| {
                let (position, size) = monitor.work_area().ok_or(WorkAreaError::QueryFailed(
                    "Windows returned an invalid monitor work area",
                ))?;
                Ok(MonitorWorkArea {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                })
            })
            .collect()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{x11, LinuxBackend, MonitorWorkArea, QueryContext, WorkAreaError};

    pub(super) fn query_work_areas(
        context: &QueryContext,
    ) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        match context.backend {
            LinuxBackend::Wayland => Err(WorkAreaError::Unsupported {
                platform: "Wayland",
                reason: "the compositor protocol does not expose monitor work areas",
            }),
            LinuxBackend::X11 => x11::query_work_areas(),
        }
    }
}

#[cfg(any(target_os = "linux", test))]
mod x11 {
    use x11rb::{
        connection::Connection,
        protocol::{
            randr::ConnectionExt as _,
            xproto::{AtomEnum, ConnectionExt as _},
        },
    };

    use super::{intersect_work_area, MonitorWorkArea, PhysicalRect, WorkAreaError};

    pub(super) fn query_work_areas() -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        let (connection, screen_index) = x11rb::connect(None)
            .map_err(|_| WorkAreaError::QueryFailed("could not connect to the X11 display"))?;
        let root = connection
            .setup()
            .roots
            .get(screen_index)
            .ok_or(WorkAreaError::QueryFailed(
                "X11 default screen index is invalid",
            ))?
            .root;
        let current_desktop_atom = intern_atom(&connection, b"_NET_CURRENT_DESKTOP")?;
        let work_area_atom = intern_atom(&connection, b"_NET_WORKAREA")?;
        let desktop_reply = connection
            .get_property(false, root, current_desktop_atom, AtomEnum::CARDINAL, 0, 1)
            .map_err(|_| WorkAreaError::QueryFailed("could not request _NET_CURRENT_DESKTOP"))?
            .reply()
            .map_err(|_| WorkAreaError::QueryFailed("could not read _NET_CURRENT_DESKTOP"))?;
        let desktop = desktop_reply
            .value32()
            .and_then(|mut values| values.next())
            .ok_or(WorkAreaError::QueryFailed(
                "_NET_CURRENT_DESKTOP is absent or malformed",
            ))? as usize;
        let work_area_reply = connection
            .get_property(false, root, work_area_atom, AtomEnum::CARDINAL, 0, u32::MAX)
            .map_err(|_| WorkAreaError::QueryFailed("could not request _NET_WORKAREA"))?
            .reply()
            .map_err(|_| WorkAreaError::QueryFailed("could not read _NET_WORKAREA"))?;
        let values = work_area_reply
            .value32()
            .ok_or(WorkAreaError::QueryFailed("_NET_WORKAREA is malformed"))?
            .collect::<Vec<_>>();
        let start = desktop
            .checked_mul(4)
            .ok_or(WorkAreaError::InvalidGeometry(
                "X11 desktop work-area index overflow",
            ))?;
        let end = start.checked_add(4).ok_or(WorkAreaError::InvalidGeometry(
            "X11 desktop work-area index overflow",
        ))?;
        let work_area = values.get(start..end).ok_or(WorkAreaError::QueryFailed(
            "_NET_WORKAREA has no entry for the current desktop",
        ))?;
        let global = PhysicalRect {
            x: work_area[0] as i32,
            y: work_area[1] as i32,
            width: work_area[2],
            height: work_area[3],
        };
        let monitors = connection
            .randr_get_monitors(root, true)
            .map_err(|_| WorkAreaError::QueryFailed("could not request RandR monitors"))?
            .reply()
            .map_err(|_| WorkAreaError::QueryFailed("could not read RandR monitors"))?
            .monitors
            .into_iter()
            .map(|monitor| PhysicalRect {
                x: i32::from(monitor.x),
                y: i32::from(monitor.y),
                width: u32::from(monitor.width),
                height: u32::from(monitor.height),
            })
            .collect::<Vec<_>>();

        intersect_work_area(global, &monitors)
    }

    fn intern_atom(
        connection: &impl Connection,
        name: &'static [u8],
    ) -> Result<u32, WorkAreaError> {
        connection
            .intern_atom(true, name)
            .map_err(|_| WorkAreaError::QueryFailed("could not request an EWMH atom"))?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|_| WorkAreaError::QueryFailed("required EWMH atom is unavailable"))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::{MonitorWorkArea, QueryContext, WorkAreaError};

    pub(super) fn query_work_areas(
        _context: &QueryContext,
    ) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
        Err(WorkAreaError::Unsupported {
            platform: std::env::consts::OS,
            reason: "no native work-area provider is implemented",
        })
    }
}

#[cfg(any(target_os = "linux", test))]
fn intersect_work_area(
    global: PhysicalRect,
    monitors: &[PhysicalRect],
) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
    if global.width == 0 || global.height == 0 {
        return Err(WorkAreaError::InvalidGeometry(
            "the desktop work area is empty",
        ));
    }
    let global_right = i64::from(global.x) + i64::from(global.width);
    let global_bottom = i64::from(global.y) + i64::from(global.height);
    let areas = monitors
        .iter()
        .filter_map(|monitor| {
            let left = i64::from(global.x).max(i64::from(monitor.x));
            let top = i64::from(global.y).max(i64::from(monitor.y));
            let right = global_right.min(i64::from(monitor.x) + i64::from(monitor.width));
            let bottom = global_bottom.min(i64::from(monitor.y) + i64::from(monitor.height));
            let width = u32::try_from(right - left)
                .ok()
                .filter(|width| *width > 0)?;
            let height = u32::try_from(bottom - top)
                .ok()
                .filter(|height| *height > 0)?;
            Some(MonitorWorkArea {
                x: i32::try_from(left).ok()?,
                y: i32::try_from(top).ok()?,
                width,
                height,
            })
        })
        .collect::<Vec<_>>();
    if areas.is_empty() {
        return Err(WorkAreaError::InvalidGeometry(
            "the desktop work area does not intersect any RandR monitor",
        ));
    }
    Ok(areas)
}

pub(crate) fn query_work_areas(
    context: &QueryContext,
) -> Result<Vec<MonitorWorkArea>, WorkAreaError> {
    platform::query_work_areas(context)
}

#[cfg(test)]
mod tests {
    use super::{intersect_work_area, x11, MonitorWorkArea, PhysicalRect, WorkAreaError};

    #[test]
    fn x11_global_work_area_is_intersected_with_negative_coordinate_monitors() {
        let _query_api: fn() -> Result<Vec<MonitorWorkArea>, WorkAreaError> = x11::query_work_areas;
        let areas = intersect_work_area(
            PhysicalRect {
                x: -1920,
                y: 24,
                width: 3840,
                height: 1056,
            },
            &[
                PhysicalRect {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
            ],
        )
        .expect("valid intersections");

        assert_eq!(areas.len(), 2);
        assert_eq!((areas[0].x, areas[0].y), (-1920, 24));
        assert_eq!((areas[1].x, areas[1].y), (0, 24));
        assert!(areas.iter().all(|area| area.height == 1056));
    }
}
