//! `path.win32` from Node: `\` and `/` both separate, `\` is written, and a
//! root is a drive (`C:`, `C:\`), a UNC share (`\\server\share\`) or `\`.

const SEP: char = '\\';

const fn is_sep(b: u8) -> bool {
    matches!(b, b'/' | b'\\')
}

/// The root of a path: the device (`C:` or `\\\\server\\share`), where the
/// rest starts, and whether it is absolute.
struct Root {
    device: Option<String>,
    end: usize,
    absolute: bool,
}

/// Index of the first byte at or after `from` whose separator-ness is not
/// `sep`.
fn skip(b: &[u8], from: usize, sep: bool) -> usize {
    b[from..]
        .iter()
        .position(|&c| is_sep(c) != sep)
        .map_or(b.len(), |n| from + n)
}

/// A `\\\\server\\share` prefix: the device and where it ends.
fn unc(path: &str) -> Option<(String, usize)> {
    let b = path.as_bytes();
    let server_end = skip(b, 2, false);
    if server_end == 2 || server_end == b.len() {
        return None;
    }
    let share_start = skip(b, server_end, true);
    if share_start == b.len() {
        return None;
    }
    let share_end = skip(b, share_start, false);
    let device = format!(
        "\\\\{}\\{}",
        &path[2..server_end],
        &path[share_start..share_end]
    );
    Some((device, share_end))
}

/// Parse the root the way `normalize` and `resolve` do.
fn root(path: &str) -> Root {
    let b = path.as_bytes();
    match b {
        [s, t, ..] if is_sep(*s) && is_sep(*t) => {
            let (device, end) = unc(path).map_or((None, 0), |(d, e)| (Some(d), e));
            Root {
                device,
                end,
                absolute: true,
            }
        }
        [s, ..] if is_sep(*s) => Root {
            device: None,
            end: 1,
            absolute: true,
        },
        [d, b':', rest @ ..] if d.is_ascii_alphabetic() => {
            let absolute = rest.first().is_some_and(|c| is_sep(*c));
            Root {
                device: Some(path[..2].to_owned()),
                end: if absolute { 3 } else { 2 },
                absolute,
            }
        }
        _ => Root {
            device: None,
            end: 0,
            absolute: false,
        },
    }
}

/// Resolve `.` and `..` segments; both separators split, `\` joins.
fn normalize_segments(path: &str, allow_above_root: bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|s| *s != "..") {
                    out.pop();
                } else if allow_above_root {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    out.join("\\")
}

/// `path.win32.normalize`.
pub fn normalize(path: &str) -> String {
    let b = path.as_bytes();
    match b {
        [] => return ".".to_owned(),
        [c] if is_sep(*c) => return SEP.to_string(),
        [_] => return path.to_owned(),
        _ => {}
    }
    let r = root(path);
    if r.end == b.len()
        && let Some(d) = r.device.as_deref().filter(|d| d.starts_with("\\\\"))
    {
        return format!("{d}{SEP}");
    }
    let mut tail = if r.end < b.len() {
        normalize_segments(&path[r.end..], !r.absolute)
    } else {
        String::new()
    };
    if tail.is_empty() && !r.absolute {
        tail.push('.');
    }
    if !tail.is_empty() && b.last().is_some_and(|c| is_sep(*c)) {
        tail.push(SEP);
    }
    match (r.device, r.absolute) {
        (None, true) => format!("{SEP}{tail}"),
        (None, false) => tail,
        (Some(d), true) => format!("{d}{SEP}{tail}"),
        (Some(d), false) => format!("{d}{tail}"),
    }
}

/// `path.win32.join`.
pub fn join(parts: &[&str]) -> String {
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    let Some(first) = kept.first().map(|f| f.as_bytes()) else {
        return ".".to_owned();
    };
    let joined = kept.join("\\");
    let leading = first.iter().take(3).take_while(|c| is_sep(**c)).count();
    let unc = leading == 2 && first.len() > 2;
    if leading == 0 || unc {
        return normalize(&joined);
    }
    let run = joined.bytes().take_while(|c| is_sep(*c)).count();
    if run >= 2 {
        normalize(&format!("{SEP}{}", &joined[run..]))
    } else {
        normalize(&joined)
    }
}

/// `path.win32.resolve(path)` with `cwd` as the working directory.
pub fn resolve(cwd: &str, path: &str) -> String {
    let mut device: Option<String> = None;
    let mut tail = String::new();
    let mut absolute = false;
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let candidates = [Some(path), Some(cwd), None];
    for candidate in candidates {
        let p: String = match candidate {
            Some("") => continue,
            Some(p) => p.to_owned(),
            None => match &device {
                None => cwd.to_owned(),
                Some(d) if cwd.get(..2).is_some_and(|c| same(c, d)) => cwd.to_owned(),
                Some(d) => format!("{d}{SEP}"),
            },
        };
        let r = root(&p);
        if let Some(d) = &r.device {
            match &device {
                Some(have) if !same(have, d) => continue,
                Some(_) => {}
                None => device = Some(d.clone()),
            }
        }
        if absolute {
            if device.is_some() {
                break;
            }
        } else {
            tail = format!("{}{SEP}{tail}", &p[r.end..]);
            absolute = r.absolute;
            if absolute && device.is_some() {
                break;
            }
        }
    }
    let tail = normalize_segments(&tail, !absolute);
    let device = device.unwrap_or_default();
    if absolute {
        format!("{device}{SEP}{tail}")
    } else if device.is_empty() && tail.is_empty() {
        ".".to_owned()
    } else {
        format!("{device}{tail}")
    }
}

/// `path.win32.basename`.
pub fn basename(path: &str) -> &str {
    let b = path.as_bytes();
    let start = if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        2
    } else {
        0
    };
    let body = path[start..].trim_end_matches(['/', '\\']);
    body.rsplit(['/', '\\']).next().unwrap_or("")
}

/// True when resolving `path` ignores the working directory: a drive root
/// (`C:\`) or a UNC path.
pub fn is_fully_qualified(path: &str) -> bool {
    let b = path.as_bytes();
    match b {
        [a, b2, ..] if is_sep(*a) && is_sep(*b2) => true,
        [d, b':', s, ..] => d.is_ascii_alphabetic() && is_sep(*s),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values from Bun's `path.win32`.
    #[test]
    fn joins_like_node() {
        let cases: [(&[&str], &str); 8] = [
            (
                &["C:\\Users\\u", ".claude", "projects"],
                "C:\\Users\\u\\.claude\\projects",
            ),
            (&["C:/a/", "b", "..", "c.jsonl"], "C:\\a\\c.jsonl"),
            (&["//srv/sh", "x"], "\\\\srv\\sh\\x"),
            (&["\\\\", "srv", "x"], "\\srv\\x"),
            (&["/", "/", "a"], "\\a"),
            (&["", ""], "."),
            (&["D:rel", "..", ".."], "D:.."),
            (&["\\\\?\\C:\\x", "y"], "\\\\?\\C:\\x\\y"),
        ];
        for (parts, want) in cases {
            assert_eq!(join(parts), want, "{parts:?}");
        }
    }

    #[test]
    fn normalizes_like_node() {
        let cases = [
            ("C:", "C:."),
            ("c:/", "c:\\"),
            ("\\\\srv\\sh", "\\\\srv\\sh\\"),
            ("//srv", "\\srv"),
            ("a\\..\\..", ".."),
            ("C:..\\x\\", "C:..\\x\\"),
            ("\\\\\\x", "\\x"),
        ];
        for (path, want) in cases {
            assert_eq!(normalize(path), want, "{path:?}");
        }
    }

    #[test]
    fn basenames_like_node() {
        assert_eq!(basename("C:\\a\\b\\"), "b");
        assert_eq!(basename("C:"), "");
        assert_eq!(basename("\\\\srv\\sh\\x"), "x");
        assert_eq!(basename("a/b"), "b");
    }

    #[test]
    fn resolves_like_node() {
        let cases = [
            ("C:\\w", "C:\\proj", "C:\\proj"),
            ("C:\\w", "\\x", "C:\\x"),
            ("C:\\w", "D:\\x\\..\\y", "D:\\y"),
            ("C:\\w", "c:rel", "c:\\w\\rel"),
            ("C:\\w", "D:rel", "D:\\rel"),
            ("\\\\srv\\sh\\d", "..\\..\\x", "\\\\srv\\sh\\x"),
            ("C:\\w", "//srv/sh/", "\\\\srv\\sh\\"),
            ("C:\\w", "", "C:\\w"),
        ];
        for (cwd, path, want) in cases {
            assert_eq!(resolve(cwd, path), want, "{path:?}");
        }
    }
}
