//! The web UI's static files: a directory on disk or the copy built into
//! the binary. Unknown paths fall back to `index.html` for client routing.

use std::path::Path;

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
    Dir(String),
    Embedded,
}

/// `path.resolve`: join onto `base` unless absolute, then drop `.` and
/// resolve `..` without touching the disk.
pub fn normalize(base: &str, path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    };
    let parts = joined.split('/').fold(Vec::new(), |mut acc, seg| {
        match seg {
            "" | "." => {}
            ".." => {
                acc.pop();
            }
            s => acc.push(s),
        }
        acc
    });
    format!("/{}", parts.join("/"))
}

impl Assets {
    /// `PEEKTRACE_CLIENT_DIR` (against `cwd`), else the built-in copy when it
    /// holds `index.html`, else the repo's `apps/inspector/dist`. Returns the
    /// source and its label.
    pub fn resolve(client_dir: Option<&str>, cwd: &str) -> (Self, String) {
        if let Some(dir) = client_dir.filter(|d| !d.is_empty()) {
            let dir = normalize(cwd, dir);
            return (Self::Dir(dir.clone()), dir);
        }
        if Embedded::get("index.html").is_some() {
            return (Self::Embedded, "embedded".to_owned());
        }
        let dist = normalize(
            "/",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/inspector/dist"),
        );
        (Self::Dir(dist.clone()), dist)
    }

    /// Answer a GET or HEAD for `raw_path` (no query).
    pub fn respond(&self, raw_path: &str) -> Response<Body> {
        let Ok(decoded) = percent_encoding::percent_decode_str(raw_path).decode_utf8() else {
            return status(StatusCode::NOT_FOUND, "");
        };
        let pathname = decoded.trim_start_matches('/');
        match self {
            Self::Dir(root) => from_dir(root, pathname),
            Self::Embedded => from_embedded(pathname),
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

fn file_response(bytes: Vec<u8>, path: &str, pathname: &str, is_index: bool) -> Response<Body> {
    let mut res = Response::new(Body::from(bytes));
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

fn from_dir(root: &str, pathname: &str) -> Response<Body> {
    let candidate = normalize(root, pathname);
    let in_root = candidate == root || candidate.starts_with(&format!("{root}/"));
    let file = (in_root && !pathname.is_empty())
        .then(|| {
            std::fs::metadata(&candidate)
                .ok()
                .filter(std::fs::Metadata::is_file)
        })
        .flatten()
        .and_then(|_| std::fs::read(&candidate).ok());
    if let Some(bytes) = file {
        return file_response(bytes, &candidate, pathname, false);
    }
    let index = format!("{root}/index.html");
    match std::fs::read(&index) {
        Ok(bytes) => file_response(bytes, &index, "", true),
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
        Some(f) if key != "index.html" => file_response(f.data.into_owned(), key, key, false),
        _ => Embedded::get("index.html").map_or_else(
            || status(StatusCode::SERVICE_UNAVAILABLE, NOT_EMBEDDED),
            |f| file_response(f.data.into_owned(), "index.html", "", true),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("/a/b", "../c"), "/a/c");
        assert_eq!(normalize("/a", "../../../x"), "/x");
        assert_eq!(normalize("/a", "./b//c/"), "/a/b/c");
        assert_eq!(normalize("/cwd", "/abs/./d"), "/abs/d");
    }

    #[test]
    fn types() {
        assert_eq!(content_type("/x/index.html"), "text/html;charset=utf-8");
        assert_eq!(content_type("a.js"), "text/javascript;charset=utf-8");
        assert_eq!(content_type("a.png"), "image/png");
        assert_eq!(content_type("a.unknownext"), "application/octet-stream");
    }
}
