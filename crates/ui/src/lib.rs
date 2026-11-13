//! peektrace views on Rune, shared by the native app and the web build.
//! Every Rune call lives in this crate, behind the default `view` feature;
//! without it only the pure `model` and chart geometry build.
#![allow(
    clippy::many_single_char_names,
    reason = "views name state s, theme t, detail d and analysis a throughout"
)]

pub mod chart;
pub mod model;
#[cfg(feature = "view")]
mod shell;
#[cfg(feature = "view")]
pub mod source;
#[cfg(feature = "view")]
pub mod state;
#[cfg(feature = "view")]
pub mod theme;
#[cfg(feature = "view")]
pub mod views;
#[cfg(feature = "view")]
pub mod web;

#[cfg(feature = "view")]
pub use shell::*;
