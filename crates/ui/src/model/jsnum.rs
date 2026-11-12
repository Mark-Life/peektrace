//! Copies of `peektrace_core::js::num` (core does not build for wasm32):
//! numbers printed and rounded the way JavaScript does. Drift tests in
//! `tests/drift.rs` compare them with core.

/// Digits and decimal exponent of a `{:e}`-formatted number.
fn split_sci(sci: &str) -> (String, i32) {
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    (digits, exp + 1)
}

/// Shortest round-trip digits of a finite, non-zero `x` and its decimal
/// exponent `n`, so that `|x| = 0.d1d2... * 10^n`. When two digit strings of
/// that length round-trip, JS takes the one closest to `x`, ties to even;
/// Rust's shortest form may take the other, so the closest one is formatted
/// exactly and used when it round-trips.
fn shortest_digits(x: f64) -> (String, i32) {
    let abs = x.abs();
    let (digits, n) = split_sci(&format!("{abs:e}"));
    let prec = digits.len().saturating_sub(1);
    let closest = format!("{abs:.prec$e}");
    if closest
        .parse::<f64>()
        .is_ok_and(|v| v.to_bits() == abs.to_bits())
    {
        let (exact, m) = split_sci(&closest);
        let exact = exact.trim_end_matches('0');
        if !exact.is_empty() && exact.len() <= digits.len() {
            return (exact.to_owned(), m);
        }
    }
    (digits, n)
}

/// `Number.prototype.toString()` (radix 10).
pub fn to_js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, n) = shortest_digits(x);
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let body = if k <= n && n <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(n - k).unwrap_or(0))
        )
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(usize::try_from(n).unwrap_or(0));
        format!("{int}.{frac}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(usize::try_from(-n).unwrap_or(0)))
    } else {
        let e = n - 1;
        let e_sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        let dot = if rest.is_empty() { "" } else { "." };
        format!("{first}{dot}{rest}e{e_sign}{}", e.abs())
    };
    format!("{sign}{body}")
}

/// A number as `JSON.stringify` writes it: non-finite values are `null`.
pub fn to_json(x: f64) -> String {
    if x.is_finite() {
        to_js_string(x)
    } else {
        "null".to_owned()
    }
}

/// `floor(|x| * 10^f)` when `|x| * 10^f` lies exactly halfway between two
/// integers, else `None`.
fn exact_tie_floor(x: f64, f: u32) -> Option<u128> {
    let bits = x.abs().to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).ok()?;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (mantissa, exp) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), biased - 1075)
    };
    if exp >= 0 {
        return None;
    }
    let shift = u32::try_from(-exp).ok()?;
    let scaled = u128::from(mantissa).checked_mul(10_u128.checked_pow(f)?)?;
    if shift >= 127 {
        return None;
    }
    let mask = (1_u128 << shift) - 1;
    (scaled & mask == 1_u128 << (shift - 1)).then_some(scaled >> shift)
}

/// `Number.prototype.toFixed(f)` for `f <= 20`. Rust rounds exact binary ties
/// to even; JS picks the larger magnitude, so `(1.25).toFixed(1)` is `1.3`.
pub fn to_fixed(x: f64, f: u32) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return to_js_string(x);
    }
    let x = if x == 0.0 { 0.0 } else { x };
    let prec = usize::try_from(f).unwrap_or(0);
    let Some(floor) = exact_tie_floor(x, f) else {
        return format!("{x:.prec$}");
    };
    let digits = (floor + 1).to_string();
    let digits = format!("{digits:0>width$}", width = prec + 1);
    let (int, frac) = digits.split_at(digits.len() - prec);
    let sign = if x < 0.0 { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// `Math.round`: the nearest integer, ties toward +Infinity.
pub fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}
