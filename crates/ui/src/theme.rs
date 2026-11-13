//! The inspector's tokens as a Rune kit, plus the fixed agent, zone and
//! chart colours.

use std::cell::RefCell;

use peektrace_protocol::AgentId;
use rune::color::hex;
use rune::{Color, TextStyle, oklch};
use rune_kit::{Kit, PaletteSeed};

use crate::model::format::Zone;

/// The app's colours on top of the kit's.
#[derive(Debug, Clone)]
pub struct Theme {
    pub kit: Kit,
    pub dark: bool,
    pub bg: Color,
    pub fg: Color,
    pub card: Color,
    pub muted: Color,
    pub muted_fg: Color,
    pub primary: Color,
    pub border: Color,
    pub sidebar: Color,
    pub sidebar_accent: Color,
    pub destructive: Color,
}

fn build(dark: bool) -> Theme {
    let (bg, fg, primary) = if dark {
        (
            oklch(0.1457, 0.0, 0.0),
            oklch(0.92, 0.0, 0.0),
            oklch(0.7869, 0.1165, 192.26),
        )
    } else {
        (
            oklch(0.9696, 0.0, 0.0),
            oklch(0.148, 0.004, 228.8),
            oklch(0.5311, 0.1062, 234.34),
        )
    };
    let mut kit = Kit::from_palette(PaletteSeed {
        bg,
        fg,
        accent: primary,
    });
    let mono = TextStyle::new(12.0, 400.0).mono();
    kit.ty.mono = mono;
    let pick = |d: Color, l: Color| if dark { d } else { l };
    Theme {
        kit,
        dark,
        bg,
        fg,
        card: pick(oklch(0.218, 0.008, 223.9), oklch(1.0, 0.0, 0.0)),
        muted: pick(oklch(0.275, 0.012, 210.0), oklch(0.963, 0.002, 197.1)),
        muted_fg: pick(oklch(0.708, 0.015, 210.0), oklch(0.49, 0.021, 213.5)),
        primary,
        border: primary.alpha(0.2),
        sidebar: pick(oklch(0.218, 0.008, 223.9), oklch(0.987, 0.002, 197.1)),
        sidebar_accent: pick(oklch(0.275, 0.011, 216.9), oklch(0.963, 0.002, 197.1)),
        destructive: pick(oklch(0.704, 0.191, 22.216), oklch(0.577, 0.245, 27.325)),
    }
}

/// The theme for `dark`, built once per thread.
pub fn theme(dark: bool) -> Theme {
    thread_local! {
        static CACHE: RefCell<[Option<Theme>; 2]> = const { RefCell::new([None, None]) };
    }
    CACHE.with(|c| {
        c.borrow_mut()[usize::from(dark)]
            .get_or_insert_with(|| build(dark))
            .clone()
    })
}

/// Parse `#rrggbb`, else grey.
pub fn hex_or_grey(s: &str) -> Color {
    hex(s).unwrap_or(Color {
        r: 0.4,
        g: 0.4,
        b: 0.4,
        a: 1.0,
    })
}

/// Agent badge colour (Tailwind 500s).
pub fn agent_color(a: AgentId) -> Color {
    hex_or_grey(match a {
        AgentId::Claude => "#f97316",
        AgentId::Codex => "#3b82f6",
        AgentId::Pi => "#06b6d4",
        AgentId::Opencode => "#8b5cf6",
    })
}

/// Text colour of a zone (Tailwind 400s).
pub fn zone_text(z: Zone) -> Color {
    hex_or_grey(match z {
        Zone::Ok => "#34d399",
        Zone::Warn => "#fbbf24",
        Zone::Bad => "#f87171",
    })
}

/// Fill colour of a zone (Tailwind 500s).
pub fn zone_fill(z: Zone) -> Color {
    hex_or_grey(match z {
        Zone::Ok => "#10b981",
        Zone::Warn => "#f59e0b",
        Zone::Bad => "#ef4444",
    })
}

pub const RED_400: &str = "#f87171";
pub const EMERALD_500: &str = "#10b981";

/// Type scale: sizes from the inspector, Geist and Geist Mono.
pub mod ty {
    use rune::TextStyle;

    pub const XS: TextStyle = TextStyle::new(12.0, 400.0);
    pub const XS_MEDIUM: TextStyle = TextStyle::new(12.0, 500.0);
    pub const SM: TextStyle = TextStyle::new(14.0, 400.0);
    pub const SM_MEDIUM: TextStyle = TextStyle::new(14.0, 500.0);
    pub const BASE_SEMI: TextStyle = TextStyle::new(16.0, 600.0);
    pub const LG_SEMI: TextStyle = TextStyle::new(18.0, 600.0);
    pub const MONO_XS: TextStyle = TextStyle::new(12.0, 400.0).mono();
    pub const MONO_11: TextStyle = TextStyle::new(11.0, 400.0).mono();
    pub const CHART_10: TextStyle = TextStyle::new(10.0, 400.0);
    pub const CHART_11: TextStyle = TextStyle::new(11.0, 400.0);
    pub const TINY: TextStyle = TextStyle::new(10.0, 500.0);
}
