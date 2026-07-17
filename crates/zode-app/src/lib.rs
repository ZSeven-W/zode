#![forbid(unsafe_code)]

pub mod app;
mod event_bridge;
pub mod event_map;
pub mod render;
pub mod services;
pub mod window_state;

pub const CRATE_READY: bool = true;
