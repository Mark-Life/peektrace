//! Number formatting and parsing with JavaScript semantics. Output must match
//! the TS CLI byte for byte, and Rust's float formatting differs from JS in
//! exponent thresholds and in how `toFixed` rounds exact ties.

use super::text;

/// Shortest round-trip digits of a finite, non-zero `x` and its decimal
/// exponent `n`, so that `|x| = 0.d1d2... * 10^n`.
fn shortest_digits(x: f64) -> (String, i32) {
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    (digits, exp + 1)
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

/// Digits of `s` in `radix`, or `None` when empty or any char is not a digit.
fn parse_radix(s: &str, radix: u32) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    s.chars().try_fold(0.0_f64, |acc, c| {
        c.to_digit(radix)
            .map(|d| acc * f64::from(radix) + f64::from(d))
    })
}

/// True for a JS `StrDecimalLiteral` without sign or `Infinity`.
fn is_decimal_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(0);
    let mut at = int;
    let mut frac = 0;
    if b.get(at) == Some(&b'.') {
        frac = digits(at + 1);
        at += 1 + frac;
    }
    if int + frac == 0 {
        return false;
    }
    if matches!(b.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(b.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exp = digits(at);
        if exp == 0 {
            return false;
        }
        at += exp;
    }
    at == b.len()
}

/// `Number(s)` for a string: JS whitespace trimmed, empty is `0`, hex/octal/
/// binary prefixes, `Infinity`, and decimal literals. Anything else is NaN.
pub fn parse_number(s: &str) -> f64 {
    let t = text::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let prefixed = [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ]
    .iter()
    .find_map(|(p, radix)| t.strip_prefix(p).map(|rest| parse_radix(rest, *radix)));
    if let Some(value) = prefixed {
        return value.unwrap_or(f64::NAN);
    }
    let (sign, unsigned) = match t.as_bytes()[0] {
        b'-' => (-1.0, &t[1..]),
        b'+' => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal_literal(unsigned) {
        return f64::NAN;
    }
    unsigned.parse::<f64>().map_or(f64::NAN, |v| sign * v)
}

/// `Math.round`: the nearest integer, ties toward +Infinity.
pub fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Number.isSafeInteger`.
#[expect(clippy::float_cmp, reason = "exact integer test")]
pub fn is_safe_integer(x: f64) -> bool {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    x.is_finite() && x.trunc() == x && x.abs() <= MAX_SAFE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_js() {
        let cases = [
            (1.708_469_600_000_000_4, "1.7084696000000004"),
            (0.0234, "0.0234"),
            (0.000_001, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123.0, "123"),
            (-0.0, "0"),
            (-2.5, "-2.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ];
        for (x, want) in cases {
            assert_eq!(to_js_string(x), want, "{x}");
        }
        assert_eq!(to_json(f64::INFINITY), "null");
        assert_eq!(to_json(f64::NAN), "null");
        assert_eq!(to_json(-2.5), "-2.5");
    }

    #[test]
    fn to_fixed_rounds_ties_up() {
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(1280.0 / 1024.0, 1), "1.3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(-1.25, 1), "-1.3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.0234, 2), "0.02");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(9.995, 2), "9.99");
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(f64::INFINITY, 2), "Infinity");
        assert_eq!(to_fixed(f64::NEG_INFINITY, 2), "-Infinity");
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
    }

    #[test]
    fn rounds_like_math_round() {
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.6), -3.0);
        assert!(js_round(f64::NAN).is_nan());
        assert_eq!(js_round(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn parses_like_number_constructor() {
        assert_eq!(parse_number("30"), 30.0);
        assert_eq!(parse_number(" 1e1 "), 10.0);
        assert_eq!(parse_number("0x10"), 16.0);
        assert_eq!(parse_number("-0").to_bits(), (-0.0_f64).to_bits());
        assert_eq!(parse_number("5."), 5.0);
        assert_eq!(parse_number(".5"), 0.5);
        assert_eq!(parse_number(""), 0.0);
        assert!(parse_number("abc").is_nan());
        assert!(parse_number("1_000").is_nan());
        assert!(parse_number("0x").is_nan());
        assert!(parse_number(".").is_nan());
        assert!(parse_number("1e").is_nan());
        assert_eq!(parse_number("-Infinity"), f64::NEG_INFINITY);
        assert!(is_safe_integer(parse_number("1e1")));
        assert!(!is_safe_integer(parse_number("1.5")));
    }
}
