//! Paint-level UI components consuming only theme tokens - a shared layer
//! beneath the widget-specific painters in `crate::widgets`. Each component
//! is a small stateless struct with a `paint` (and sometimes `min_width`)
//! associated function; none hold widget state or hit-testing logic.
//!
//! This module is deliberately not yet consumed by most existing widgets.
//! The settings/provider_models buttons and inputs are migrated as the
//! first two proof call sites (see
//! `widgets/settings/provider_models/paint_helpers.rs`); migrating the
//! rest of the crate's hand-rolled buttons/inputs/menu rows is left to a
//! follow-up sweep.

mod button;
mod card;
mod icon_button;
mod input;
mod menu_row;
mod pill;

#[cfg(test)]
mod test_support;

pub use button::{Button, ButtonVariant};
pub use card::Card;
pub use icon_button::IconButton;
pub use input::Input;
pub use menu_row::MenuRow;
pub use pill::Pill;
