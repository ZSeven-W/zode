#![deny(unsafe_code)]

#[allow(unsafe_code)]
pub mod accessibility_host;
pub mod app;
mod bootstrap_state;
pub mod clipboard;
mod command_bridge;
mod command_projection;
pub mod cursor;
mod event_bridge;
pub mod event_map;
mod ime;
pub mod input_dispatch;
mod platform_work_area;
pub mod preferences;
pub mod presentation_bridge;
#[allow(unsafe_code)]
mod presenter;
pub mod render;
pub mod services;
pub mod terminal_runtime;
pub mod window_bootstrap;
pub mod window_state;
pub mod work_area;

pub const CRATE_READY: bool = true;
