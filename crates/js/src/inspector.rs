//! How the inspector spells numbers in a browser, for every surface that
//! must print the same text: core's stats sentences and the Rune views.

use crate::num::{js_round, to_fixed, to_js_string};

const SEC_PER_MIN: f64 = 60.0;
const SEC_PER_HOUR: f64 = 3600.0;
const THOUSAND: f64 = 1000.0;
const MILLION: f64 = 1_000_000.0;

fn pad2(s: String) -> String {
    if s.len() < 2 { format!("0{s}") } else { s }
}

/// `4h12m`, `3m07s`, `0.42s`. A minute rounds its seconds, so `119.6` is
/// `1m60s`, as the inspector prints it.
pub fn hms(sec: f64) -> String {
    if sec < SEC_PER_MIN {
        let digits = if sec < 10.0 { 2 } else { 0 };
        return format!("{}s", to_fixed(sec, digits));
    }
    if sec < SEC_PER_HOUR {
        let m = (sec / SEC_PER_MIN).floor();
        let s = js_round(sec - m * SEC_PER_MIN);
        return format!("{}m{}s", to_js_string(m), pad2(to_js_string(s)));
    }
    let h = (sec / SEC_PER_HOUR).floor();
    let m = ((sec - h * SEC_PER_HOUR) / SEC_PER_MIN).floor();
    format!("{}h{}m", to_js_string(h), pad2(to_js_string(m)))
}

/// A 0-1 share as a percentage with two decimals.
pub fn pct(share: f64) -> String {
    format!("{}%", to_fixed(share * 100.0, 2))
}

/// Compact: `1.2K`, `1M`; whole thousands and millions drop the decimal.
pub fn fmt_k(n: f64) -> String {
    let scaled = |unit: f64, suffix: &str| {
        let digits = u32::from(n % unit != 0.0);
        format!("{}{suffix}", to_fixed(n / unit, digits))
    };
    if n >= MILLION {
        scaled(MILLION, "M")
    } else if n >= THOUSAND {
        scaled(THOUSAND, "K")
    } else {
        to_js_string(js_round(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inspector's TS formatters read the same file.
    const SPELLING: &str = include_str!("../../../tests/fixtures/stats/number-spelling.json");

    fn cases(key: &str) -> Vec<(f64, String)> {
        let v: serde_json::Value = serde_json::from_str(SPELLING).expect("spelling json");
        v[key]
            .as_array()
            .expect("cases")
            .iter()
            .map(|c| {
                (
                    c[0].as_f64().expect("input"),
                    c[1].as_str().expect("want").to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn spelled_as_the_inspector_spells_them() {
        for (x, want) in cases("hms") {
            assert_eq!(hms(x), want, "hms {x}");
        }
        for (x, want) in cases("pct") {
            assert_eq!(pct(x), want, "pct {x}");
        }
        for (x, want) in cases("fmtK") {
            assert_eq!(fmt_k(x), want, "fmt_k {x}");
        }
    }
}
