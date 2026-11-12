//! Node `os` helpers whose output reaches JSON.

/// `os.tmpdir()` from the variables `var` reads: one trailing separator is
/// dropped, unlike `std::env::temp_dir()`.
pub fn tmpdir_of(windows: bool, var: impl Fn(&str) -> Option<String>) -> String {
    let set = |k: &str| var(k).filter(|v| !v.is_empty());
    if windows {
        let path = set("TEMP").or_else(|| set("TMP")).unwrap_or_else(|| {
            let root = set("SystemRoot")
                .or_else(|| set("windir"))
                .unwrap_or_default();
            format!("{root}\\temp")
        });
        if path.len() > 1 && path.ends_with('\\') && !path.ends_with(":\\") {
            return path[..path.len() - 1].to_owned();
        }
        return path;
    }
    let path = set("TMPDIR")
        .or_else(|| set("TMP"))
        .or_else(|| set("TEMP"))
        .unwrap_or_else(|| "/tmp".to_owned());
    match path.strip_suffix('/') {
        Some(p) if path.len() > 1 => p.to_owned(),
        _ => path,
    }
}

/// `os.tmpdir()` for this process.
pub fn tmpdir() -> String {
    tmpdir_of(cfg!(windows), |k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posix(vars: &[(&str, &str)]) -> String {
        tmpdir_of(false, |k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| (*v).to_owned())
        })
    }

    #[test]
    fn drops_one_trailing_slash() {
        assert_eq!(
            posix(&[("TMPDIR", "/var/folders/x/T/")]),
            "/var/folders/x/T"
        );
        assert_eq!(posix(&[("TMPDIR", "/a//")]), "/a/");
        assert_eq!(posix(&[("TMPDIR", "/")]), "/");
        assert_eq!(posix(&[("TMPDIR", ""), ("TMP", "/t")]), "/t");
        assert_eq!(posix(&[]), "/tmp");
    }

    #[test]
    fn windows_keeps_a_drive_root() {
        let w = |v: &str| {
            let v = v.to_owned();
            tmpdir_of(true, move |k| (k == "TEMP").then(|| v.clone()))
        };
        assert_eq!(w(r"C:\Temp\"), r"C:\Temp");
        assert_eq!(w(r"C:\"), r"C:\");
    }
}
