//! Context-health verdict bands shared by `sessions analyze` and
//! `statusline`.

/// Fraction at or above which a session is judged to be rotting.
pub const ROTTING_FRACTION: f64 = 0.7;

/// Share of the window, 0 when the window is unknown.
pub fn fraction_of(tokens: f64, window: f64) -> f64 {
    if window > 0.0 { tokens / window } else { 0.0 }
}

pub fn verdict(fraction: f64, dumb_zone: f64) -> &'static str {
    if fraction < dumb_zone {
        "Healthy"
    } else if fraction < ROTTING_FRACTION {
        "Degrading"
    } else {
        "Rotting"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        assert_eq!(verdict(0.1, 0.4), "Healthy");
        assert_eq!(verdict(0.5, 0.4), "Degrading");
        assert_eq!(verdict(0.7, 0.4), "Rotting");
    }

    /// TS `verdict.test.ts`.
    #[test]
    fn verdict_bands_switch_at_the_dumb_zone_and_at_rotting() {
        assert_eq!(verdict(0.39, 0.4), "Healthy");
        assert_eq!(verdict(0.4, 0.4), "Degrading");
        assert_eq!(verdict(ROTTING_FRACTION - 0.01, 0.4), "Degrading");
        assert_eq!(verdict(ROTTING_FRACTION, 0.4), "Rotting");
    }

    /// TS `verdict.test.ts`.
    #[test]
    #[allow(clippy::float_cmp)]
    fn fractions_are_zero_for_an_unknown_window() {
        assert_eq!(fraction_of(5.0, 0.0), 0.0);
        assert_eq!(fraction_of(2.0, 10.0), 0.2);
    }
}
