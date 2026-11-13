//! The web UI's static files: a directory on disk or the copy built into
//! the binary. Unknown paths fall back to `index.html` for client routing.

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderValue, Response, StatusCode, header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/inspector/dist"]
#[allow_missing = true]
struct Embedded;

const NOT_BUILT: &str = "Inspector assets not built. Run: cd apps/inspector && bun run build";
const NOT_EMBEDDED: &str = "Inspector assets not embedded.";

/// Where the UI files come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assets {
    /// An absolute, normalized directory.
    Dir(PathBuf),
    Embedded,
    /// The Rune web app (`serve --ui rune`).
    Rune(crate::rune_assets::RuneSource),
}

/// `path.resolve`: join onto `base` unless absolute, then drop `.` and
/// resolve `..` without touching the disk.
pub fn normalize(base: &Path, path: &Path) -> PathBuf {
    base.join(path)
        .components()
        .fold(PathBuf::new(), |mut acc, c| {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    acc.pop();
                }
                c => acc.push(c),
            }
            acc
        })
}

/// A URL path as a path under the UI root, or `None` when it leaves the
/// root or a segment holds a separator or drive mark another OS would read.
pub(crate) fn under_root(pathname: &str) -> Option<PathBuf> {
    pathname
        .split('/')
        .try_fold(Vec::new(), |mut acc, seg| {
            match seg {
                "" | "." => {}
                ".." => {
                    acc.pop()?;
                }
                s if s.contains(['\\', ':', '\0']) => return None,
                s => acc.push(s),
            }
            Some(acc)
        })
        .map(|segs| segs.into_iter().collect())
}

/// The repo's built UI, for debug builds run from a checkout. Release builds
/// keep no build-machine path.
#[cfg(debug_assertions)]
#[expect(clippy::unnecessary_wraps, reason = "release builds have none")]
fn dev_dist() -> Option<PathBuf> {
    Some(normalize(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new("../../apps/inspector/dist"),
    ))
}

#[cfg(not(debug_assertions))]
const fn dev_dist() -> Option<PathBuf> {
    None
}

impl Assets {
    /// `PEEKTRACE_CLIENT_DIR` (against `cwd`), else the built-in copy when it
    /// holds `index.html`, else, in debug builds, the repo's
    /// `apps/inspector/dist`. Returns the source and its label.
    pub fn resolve(client_dir: Option<&str>, cwd: &str) -> (Self, String) {
        if let Some(dir) = client_dir.filter(|d| !d.is_empty()) {
            let dir = normalize(Path::new(cwd), Path::new(dir));
            let label = dir.display().to_string();
            return (Self::Dir(dir), label);
        }
        let embedded = || (Self::Embedded, "embedded".to_owned());
        if Embedded::get("index.html").is_some() {
            return embedded();
        }
        dev_dist().map_or_else(embedded, |dist| {
            let label = dist.display().to_string();
            (Self::Dir(dist), label)
        })
    }

    /// Answer a GET or HEAD for `raw_path` (no query). The Rune app reads
    /// the request headers for brotli and `If-None-Match`. Blocks on disk.
    pub fn respond_to(&self, raw_path: &str, req: &axum::http::HeaderMap) -> Response<Body> {
        let Ok(decoded) = percent_encoding::percent_decode_str(raw_path).decode_utf8() else {
            return status(StatusCode::NOT_FOUND, "");
        };
        let pathname = decoded.trim_start_matches('/');
        match self {
            Self::Dir(root) => from_dir(root, pathname),
            Self::Embedded => from_embedded(pathname),
            Self::Rune(src) => src.respond(pathname, req),
        }
    }
}

fn status(code: StatusCode, body: &'static str) -> Response<Body> {
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = code;
    if !body.is_empty() {
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain;charset=utf-8"),
        );
    }
    res
}

/// Media type by extension, with the charset Bun adds to text types.
fn content_type(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let media = match ext {
        "js" | "mjs" => "text/javascript".to_owned(),
        _ => mime_guess::from_ext(ext)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
    };
    if media.starts_with("text/") || media == "application/json" {
        format!("{media};charset=utf-8")
    } else {
        media
    }
}

fn file_response(
    bytes: impl Into<Body>,
    path: &str,
    pathname: &str,
    is_index: bool,
) -> Response<Body> {
    let mut res = Response::new(bytes.into());
    let headers = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&content_type(path)) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if is_index {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    } else if pathname.starts_with("assets/") {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    res
}

fn from_dir(root: &Path, pathname: &str) -> Response<Body> {
    let file = under_root(pathname)
        .filter(|rel| rel.components().next().is_some())
        .map(|rel| root.join(rel))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
        .and_then(|p| std::fs::read(p).ok());
    if let Some(bytes) = file {
        return file_response(bytes, pathname, pathname, false);
    }
    match std::fs::read(root.join("index.html")) {
        Ok(bytes) => file_response(bytes, "index.html", "", true),
        Err(_) => status(StatusCode::SERVICE_UNAVAILABLE, NOT_BUILT),
    }
}

fn from_embedded(pathname: &str) -> Response<Body> {
    let key = if pathname.is_empty() {
        "index.html"
    } else {
        pathname
    };
    match Embedded::get(key) {
        Some(f) if key != "index.html" => file_response(f.data, key, key, false),
        _ => Embedded::get("index.html").map_or_else(
            || status(StatusCode::SERVICE_UNAVAILABLE, NOT_EMBEDDED),
            |f| file_response(f.data, "index.html", "", true),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(base: &str, path: &str) -> String {
        normalize(Path::new(base), Path::new(path))
            .display()
            .to_string()
    }

    #[cfg(unix)]
    #[test]
    fn normalizes() {
        assert_eq!(norm("/a/b", "../c"), "/a/c");
        assert_eq!(norm("/a", "../../../x"), "/x");
        assert_eq!(norm("/a", "./b//c/"), "/a/b/c");
        assert_eq!(norm("/cwd", "/abs/./d"), "/abs/d");
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_windows_paths() {
        assert_eq!(norm(r"C:\work", r"C:\ui"), r"C:\ui");
        assert_eq!(norm(r"C:\work", "dist"), r"C:\work\dist");
        assert_eq!(
            norm(r"C:\repo\crates\server", "../../apps/inspector/dist"),
            r"C:\repo\apps\inspector\dist"
        );
    }

    #[test]
    fn request_paths_stay_under_the_root() {
        let rel = |p: &str| under_root(p).map(|r| r.display().to_string());
        assert_eq!(
            rel("assets/a.js"),
            Some(Path::new("assets").join("a.js").display().to_string())
        );
        assert_eq!(rel("a/../b.css"), Some("b.css".to_owned()));
        assert_eq!(rel("../secret"), None);
        assert_eq!(rel("..\\..\\secret"), None);
        assert_eq!(rel("C:secret"), None);
        assert_eq!(rel(""), Some(String::new()));
    }

    #[test]
    fn types() {
        assert_eq!(content_type("/x/index.html"), "text/html;charset=utf-8");
        assert_eq!(content_type("a.js"), "text/javascript;charset=utf-8");
        assert_eq!(content_type("a.png"), "image/png");
        assert_eq!(content_type("a.unknownext"), "application/octet-stream");
    }
}
