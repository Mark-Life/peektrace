//! How a model id finds its rate card key: the key sits on a boundary in the
//! id, and only date stamps, Bedrock versions and `-latest` may follow it.

/// Newest generation per Anthropic family, for a bare alias like `claude-opus`.
const FAMILY_FALLBACK: &[(&str, &str)] = &[
    ("opus", "opus-5-5"),
    ("sonnet", "sonnet-5-5"),
    ("haiku", "haiku-5-5"),
    ("fable", "fable-5-2"),
    ("mythos", "mythos-5-1"),
];

const fn is_boundary(b: u8) -> bool {
    matches!(b, b'/' | b'.' | b'-' | b'_' | b':')
}

/// `n` ASCII digits at the start of `s`, if present.
fn digits(s: &[u8], n: usize) -> Option<&[u8]> {
    let head = s.get(..n)?;
    head.iter().all(u8::is_ascii_digit).then(|| &s[n..])
}

/// Every remainder left after one `SUFFIX` match at the start of `s`.
fn one_suffix(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    if let Some(rest) = s.strip_prefix(b"-").and_then(|r| digits(r, 8)) {
        out.push(rest);
    }
    if let Some(rest) = s
        .strip_prefix(b"-")
        .and_then(|r| digits(r, 4))
        .and_then(|r| r.strip_prefix(b"-"))
        .and_then(|r| digits(r, 2))
        .and_then(|r| r.strip_prefix(b"-"))
        .and_then(|r| digits(r, 2))
    {
        out.push(rest);
    }
    if let Some(rest) = s.strip_prefix(b"@").and_then(|r| digits(r, 8)) {
        out.push(rest);
    }
    if let Some(rest) = s.strip_prefix(b"-v") {
        let run = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        for n in 1..=run {
            let after = &rest[n..];
            out.push(after);
            if let Some(colon) = after.strip_prefix(b":") {
                let run2 = colon.iter().take_while(|b| b.is_ascii_digit()).count();
                out.extend((1..=run2).map(|m| &colon[m..]));
            }
        }
    }
    if let Some(rest) = s.strip_prefix(b"-latest") {
        out.push(rest);
    }
    out
}

/// True when `s` is a run of date stamps, Bedrock versions and `-latest`
/// (`^(?:SUFFIX)*$`).
fn only_suffixes(s: &[u8]) -> bool {
    s.is_empty() || one_suffix(s).into_iter().any(only_suffixes)
}

/// True when `key` sits on a boundary in `id` and only allowed suffixes follow.
pub(super) fn key_matches(id: &str, key: &str) -> bool {
    let bytes = id.as_bytes();
    let mut from = 0;
    while let Some(found) = id.get(from..).and_then(|rest| rest.find(key)) {
        let at = from + found;
        let before = at == 0 || is_boundary(bytes[at - 1]);
        if before && only_suffixes(&bytes[at + key.len()..]) {
            return true;
        }
        from = at + 1;
    }
    false
}

/// `dashed` with any trailing suffix run removed (`SUFFIX*$` replaced).
pub(super) fn strip_trailing_suffixes(dashed: &str) -> &str {
    let bytes = dashed.as_bytes();
    (0..=bytes.len())
        .find(|&p| dashed.is_char_boundary(p) && only_suffixes(&bytes[p..]))
        .map_or(dashed, |p| &dashed[..p])
}

/// The family key a bare, version-free alias names.
pub(super) fn alias_family(dashed: &str) -> Option<&'static str> {
    let stem = strip_trailing_suffixes(dashed);
    if stem.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    FAMILY_FALLBACK.iter().find_map(|&(family, key)| {
        let at = stem.len().checked_sub(family.len())?;
        let fits = stem.ends_with(family) && (at == 0 || is_boundary(stem.as_bytes()[at - 1]));
        fits.then_some(key)
    })
}
