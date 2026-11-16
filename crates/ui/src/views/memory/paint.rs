//! The fixed colours the memory views use (Tailwind steps and the
//! `--chart-N` tokens, the same in both themes).

use rune::{Color, TextStyle, oklch};

use crate::chart::memory::BarTone;
use crate::theme::hex_or_grey;

pub const MONO_XS_SEMI: TextStyle = TextStyle::new(12.0, 600.0).mono();

pub fn red_400() -> Color {
    hex_or_grey("#f87171")
}

pub fn red_500() -> Color {
    hex_or_grey("#ef4444")
}

pub fn amber_300() -> Color {
    hex_or_grey("#fcd34d")
}

pub fn amber_400() -> Color {
    hex_or_grey("#fbbf24")
}

pub fn amber_500() -> Color {
    hex_or_grey("#f59e0b")
}

pub fn emerald_400() -> Color {
    hex_or_grey("#34d399")
}

pub fn emerald_500() -> Color {
    hex_or_grey("#10b981")
}

pub fn sky_400() -> Color {
    hex_or_grey("#38bdf8")
}

pub fn tone_color(t: BarTone) -> Color {
    match t {
        BarTone::Ok => emerald_500(),
        BarTone::Warn => amber_500(),
        BarTone::Over => red_500(),
    }
}

/// `--chart-1` to `--chart-5`.
pub fn chart_color(n: usize) -> Color {
    match n {
        1 => oklch(0.865, 0.127, 207.078),
        2 => oklch(0.715, 0.143, 215.221),
        3 => oklch(0.609, 0.126, 221.723),
        4 => oklch(0.52, 0.105, 223.128),
        _ => oklch(0.45, 0.085, 224.283),
    }
}
