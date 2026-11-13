//! Palette, per-agent and per-level tables, the health zone, and the colour
//! mode a terminal gets: truecolor, the xterm-256 cube, or none.

use peektrace_core::Agent;
use peektrace_core::capabilities::SupportLevel;
use peektrace_core::sessions::model::AnalyzedSession;
use peektrace_core::sessions::model::Provider;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

const fn rgb(hex: u32) -> Color {
    #[expect(clippy::cast_possible_truncation, reason = "byte extraction")]
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const BG: Color = rgb(0x0b_0e14);
pub const PANEL_SEL: Color = rgb(0x18_2030);
pub const BORDER: Color = rgb(0x23_2a36);
pub const BORDER_ACTIVE: Color = rgb(0x3d_6fd0);
pub const TEXT: Color = rgb(0xc9_d1d9);
pub const TEXT_DIM: Color = rgb(0x8b_949e);
pub const TEXT_FAINT: Color = rgb(0x6e_7681);
pub const PRIMARY: Color = rgb(0x6a_a0ff);
pub const ACCENT: Color = rgb(0x39_d0d8);
pub const GOOD: Color = rgb(0x3f_b950);
pub const WARN: Color = rgb(0xd2_9922);
pub const BAD: Color = rgb(0xf8_5149);
pub const INFO: Color = rgb(0x58_a6ff);
pub const ON_PRIMARY: Color = rgb(0x0b_0e14);

/// Foreground-only style.
pub fn fg(c: Color) -> Style {
    Style::new().fg(c)
}

/// A `#rrggbb` colour from core (budget slices); text colour when malformed.
pub fn hex(s: &str) -> Color {
    s.strip_prefix('#')
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .map_or(TEXT, rgb)
}

pub const fn agent_label(a: Agent) -> &'static str {
    match a {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
        Agent::Pi => "Pi",
        Agent::Opencode => "OpenCode",
    }
}

pub const fn agent_color(a: Agent) -> Color {
    match a {
        Agent::Claude => rgb(0xcc_785c),
        Agent::Codex => rgb(0x10_a37f),
        Agent::Pi => rgb(0xb0_8cff),
        Agent::Opencode => rgb(0xf2_b705),
    }
}

pub const fn provider_label(p: Provider) -> &'static str {
    match p {
        Provider::ClaudeCode => "Claude Code",
        Provider::Codex => "Codex",
        Provider::Pi => "Pi",
        Provider::Opencode => "OpenCode",
    }
}

pub const fn level_color(l: SupportLevel) -> Color {
    match l {
        SupportLevel::Supported => GOOD,
        SupportLevel::Partial => WARN,
        SupportLevel::Planned => INFO,
        SupportLevel::Unsupported => TEXT_FAINT,
    }
}

pub const fn level_gloss(l: SupportLevel) -> &'static str {
    match l {
        SupportLevel::Supported => "Built and working today.",
        SupportLevel::Partial => "Partially built — some paths work.",
        SupportLevel::Planned => "On the roadmap, not yet built.",
        SupportLevel::Unsupported => "Not applicable / no plan.",
    }
}

/// Peak-context health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Ok,
    Warn,
    Bad,
}

/// Dumb-zone floor below which the warn threshold is pinned at 75%.
const WARN_FLOOR: f64 = 0.75;

pub fn zone_of(s: &AnalyzedSession) -> Zone {
    let frac = if s.context_window > 0.0 {
        s.peak_context_tokens / s.context_window
    } else {
        0.0
    };
    let dz = s.dumb_zone_fraction;
    let warn = if dz < WARN_FLOOR {
        WARN_FLOOR
    } else {
        f64::midpoint(1.0, dz)
    };
    if frac < dz {
        Zone::Ok
    } else if frac < warn {
        Zone::Warn
    } else {
        Zone::Bad
    }
}

pub const fn verdict(z: Zone) -> (&'static str, Color) {
    match z {
        Zone::Ok => ("Healthy", GOOD),
        Zone::Warn => ("Degrading", WARN),
        Zone::Bad => ("Rotting", BAD),
    }
}

/// How colours reach the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    TrueColor,
    Ansi256,
    /// `NO_COLOR`: selection and focus drawn with reverse and bold.
    None,
}

impl ColorMode {
    /// From `NO_COLOR` and `COLORTERM`.
    pub fn detect(var: impl Fn(&str) -> Option<String>) -> Self {
        if var("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            Self::None
        } else if var("COLORTERM").is_some_and(|v| v == "truecolor" || v == "24bit") {
            Self::TrueColor
        } else {
            Self::Ansi256
        }
    }
}

/// Nearest xterm-256 index for an RGB colour (6x6x6 cube or grey ramp).
pub fn to_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let near = |v: u8| {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| (i32::from(**l) - i32::from(v)).abs())
            .map_or(0, |(i, _)| i)
    };
    let (ri, gi, bi) = (near(r), near(g), near(b));
    #[expect(clippy::cast_possible_truncation, reason = "indexes are below 6")]
    let cube = 16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8;
    let dist = |x: (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(x.0, r) + d(x.1, g) + d(x.2, b)
    };
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let grey_i = (avg.saturating_sub(8) / 10).min(23);
    #[expect(clippy::cast_possible_truncation, reason = "grey index is below 24")]
    let grey_v = (8 + grey_i * 10) as u8;
    let cube_rgb = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    if dist((grey_v, grey_v, grey_v)) < dist(cube_rgb) {
        232 + u8::try_from(grey_i).unwrap_or(23)
    } else {
        cube
    }
}

fn map_color(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Indexed(to_256(r, g, b)),
        other => other,
    }
}

/// Rewrite a drawn buffer for the colour mode. With no colour, cells on a
/// selection or active-tab background turn reversed and focused borders bold.
pub fn apply(buf: &mut Buffer, mode: ColorMode) {
    match mode {
        ColorMode::TrueColor => {}
        ColorMode::Ansi256 => {
            for cell in &mut buf.content {
                cell.fg = map_color(cell.fg);
                cell.bg = map_color(cell.bg);
            }
        }
        ColorMode::None => {
            for cell in &mut buf.content {
                if cell.bg == PANEL_SEL || cell.bg == PRIMARY {
                    cell.modifier |= Modifier::REVERSED;
                }
                if cell.fg == BORDER_ACTIVE {
                    cell.modifier |= Modifier::BOLD;
                }
                cell.fg = Color::Reset;
                cell.bg = Color::Reset;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_grey() {
        assert_eq!(to_256(0, 0, 0), 16);
        assert_eq!(to_256(255, 255, 255), 231);
        assert_eq!(to_256(0x6e, 0x76, 0x81), 243);
        assert_eq!(to_256(0xff, 0, 0), 196);
    }

    #[test]
    fn color_mode_from_env() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            ColorMode::detect(env(&[("NO_COLOR", "1")])),
            ColorMode::None
        );
        assert_eq!(
            ColorMode::detect(env(&[("NO_COLOR", "")])),
            ColorMode::Ansi256
        );
        assert_eq!(
            ColorMode::detect(env(&[("COLORTERM", "truecolor")])),
            ColorMode::TrueColor
        );
    }
}
