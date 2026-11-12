//! `path.posix` from Node.

/// Resolve `.` and `..` segments and collapse repeated separators.
fn normalize_segments(path: &str, allow_above_root: bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
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
    out.join("/")
}

/// `path.posix.normalize`.
fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".to_owned();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut body = normalize_segments(path, !absolute);
    if body.is_empty() && !absolute {
        body.push('.');
    }
    if !body.is_empty() && trailing {
        body.push('/');
    }
    if absolute { format!("/{body}") } else { body }
}

/// `path.posix.join`.
pub fn join(parts: &[&str]) -> String {
    let joined = parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    if joined.is_empty() {
        ".".to_owned()
    } else {
        normalize(&joined)
    }
}

/// `path.posix.resolve(path)` against `cwd`.
pub fn resolve(cwd: &str, path: &str) -> String {
    let full = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{cwd}/{path}")
    };
    let absolute = full.starts_with('/');
    let body = normalize_segments(&full, !absolute);
    match (absolute, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".to_owned(),
        (false, false) => body,
    }
}

/// `path.posix.dirname` of a path with no trailing slash.
pub fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(at) => path[..at].to_owned(),
        None => ".".to_owned(),
    }
}

/// `path.posix.basename`.
pub fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_like_node() {
        assert_eq!(join(&["/a/", "b", "c.jsonl"]), "/a/b/c.jsonl");
        assert_eq!(join(&["/a//b/", "../c"]), "/a/c");
        assert_eq!(join(&["a", "./b"]), "a/b");
        assert_eq!(join(&["", "a"]), "a");
        assert_eq!(join(&["", ""]), ".");
        assert_eq!(join(&["../a", ".."]), "..");
        assert_eq!(join(&["/", ".."]), "/");
        assert_eq!(join(&["a/", ""]), "a/");
    }

    #[test]
    fn resolves_like_node() {
        assert_eq!(resolve("/home/u", "x/../y/"), "/home/u/y");
        assert_eq!(resolve("/home/u", "/a//b/."), "/a/b");
        assert_eq!(resolve("/home/u", ""), "/home/u");
        assert_eq!(resolve("/", ".."), "/");
    }

    #[test]
    fn basenames_like_node() {
        assert_eq!(basename("/a/b/"), "b");
        assert_eq!(basename("/"), "");
        assert_eq!(basename("work"), "work");
    }
}
