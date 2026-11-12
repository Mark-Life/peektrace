//! The Rune web app's files (`serve --ui rune`): a directory on disk, the
//! copy built in with the `rune-ui` feature, or the checkout's
//! `crates/app/dist` in debug builds. File names carry no hash, so every
//! file but the page revalidates by `ETag`; a `.br` twin is sent to clients
//! that accept brotli.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use sha2::{Digest as _, Sha256};

use crate::assets::{normalize, under_root};

#[cfg(feature = "rune-ui")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../crates/app/dist"]
#[allow_missing = true]
struct Embedded;

const NOT_BUILT: &str = "Rune UI not built. Run: scripts/rust/build-rune-web.sh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuneSource {
    Dir(PathBuf),
    Embedded,
    Missing,
}

#[cfg(debug_assertions)]
fn dev_dist() -> Option<PathBuf> {
    let dist = normalize(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new("../app/dist"),
    );
    dist.join("index.html").is_file().then_some(dist)
}

#[cfg(not(debug_assertions))]
const fn dev_dist() -> Option<PathBuf> {
    None
}

#[cfg(feature = "rune-ui")]
fn embedded_get(path: &str) -> Option<Vec<u8>> {
    Embedded::get(path).map(|f| f.data.into_owned())
}

#[cfg(not(feature = "rune-ui"))]
const fn embedded_get(_path: &str) -> Option<Vec<u8>> {
    None
}

impl RuneSource {
    /// `PEEKTRACE_RUNE_DIR` (against `cwd`), else the built-in copy, else
    /// (debug builds) the checkout's dist. Returns the source and its label.
    pub fn resolve(rune_dir: Option<&str>, cwd: &str) -> (Self, String) {
        if let Some(dir) = rune_dir.filter(|d| !d.is_empty()) {
            let dir = normalize(Path::new(cwd), Path::new(dir));
            let label = dir.display().to_string();
            return (Self::Dir(dir), label);
        }
        if embedded_get("index.html").is_some() {
            return (Self::Embedded, "embedded".to_owned());
        }
        dev_dist().map_or_else(
            || (Self::Missing, "not built".to_owned()),
            |d| {
                let label = d.display().to_string();
                (Self::Dir(d), label)
            },
        )
    }

    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match self {
            Self::Dir(root) => {
                let path = under_root(rel).filter(|r| r.components().next().is_some())?;
                let full = root.join(path);
                std::fs::metadata(&full)
                    .ok()
                    .filter(std::fs::Metadata::is_file)?;
                std::fs::read(full).ok()
            }
            Self::Embedded => embedded_get(rel),
            Self::Missing => None,
        }
    }

    /// Answer a GET or HEAD for the decoded path, given the request headers.
    pub fn respond(&self, pathname: &str, req: &HeaderMap) -> Response<Body> {
        if *self == Self::Missing {
            return plain(StatusCode::SERVICE_UNAVAILABLE, NOT_BUILT);
        }
        let rel = pathname.trim_start_matches('/');
        let is_twin = Path::new(rel).extension().is_some_and(|e| e == "br");
        let file = (!rel.is_empty() && rel != "index.html" && !is_twin)
            .then(|| self.read(rel))
            .flatten();
        let Some(bytes) = file else {
            if is_twin {
                return plain(StatusCode::NOT_FOUND, "");
            }
            return self.read("index.html").map_or_else(
                || plain(StatusCode::SERVICE_UNAVAILABLE, NOT_BUILT),
                |b| {
                    let mut res = Response::new(Body::from(b));
                    set(&mut res, header::CONTENT_TYPE, "text/html;charset=utf-8");
                    set(&mut res, header::CACHE_CONTROL, "no-store");
                    res
                },
            );
        };
        let twin = accepts_br(req)
            .then(|| self.read(&format!("{rel}.br")))
            .flatten();
        let (body, encoded) = twin.map_or((bytes, false), |b| (b, true));
        let etag = format!("\"{}{}\"", hash16(&body), if encoded { "-br" } else { "" });
        let fresh = req
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
        let mut res = if fresh {
            let mut r = Response::new(Body::empty());
            *r.status_mut() = StatusCode::NOT_MODIFIED;
            r
        } else {
            Response::new(Body::from(body))
        };
        set(&mut res, header::CONTENT_TYPE, content_type(rel));
        set(&mut res, header::CACHE_CONTROL, "no-cache");
        set(&mut res, header::VARY, "accept-encoding");
        if let Ok(v) = HeaderValue::from_str(&etag) {
            res.headers_mut().insert(header::ETAG, v);
        }
        if encoded {
            set(&mut res, header::CONTENT_ENCODING, "br");
        }
        res
    }
}

fn set(res: &mut Response<Body>, name: header::HeaderName, value: &'static str) {
    res.headers_mut()
        .insert(name, HeaderValue::from_static(value));
}

fn plain(code: StatusCode, body: &'static str) -> Response<Body> {
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = code;
    if !body.is_empty() {
        set(&mut res, header::CONTENT_TYPE, "text/plain;charset=utf-8");
    }
    res
}

/// First 16 hex digits of the SHA-256 of `bytes`.
fn hash16(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// `Accept-Encoding` lists `br` with a non-zero q.
pub fn accepts_br(req: &HeaderMap) -> bool {
    req.get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|part| {
            let mut it = part.split(';');
            let name = it.next().unwrap_or_default().trim();
            let q = it
                .find_map(|p| p.trim().strip_prefix("q=").map(str::trim))
                .map_or(1.0, |q| q.parse::<f64>().unwrap_or(0.0));
            name.eq_ignore_ascii_case("br") && q > 0.0
        })
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "wasm" => "application/wasm",
        "js" | "mjs" => "text/javascript;charset=utf-8",
        "ttf" => "font/ttf",
        "html" => "text/html;charset=utf-8",
        "json" => "application/json;charset=utf-8",
        "css" => "text/css;charset=utf-8",
        "ts" => "text/plain;charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    header::HeaderName::from_static(k),
                    HeaderValue::from_static(v),
                )
            })
            .collect()
    }

    #[test]
    fn brotli_acceptance() {
        assert!(accepts_br(&headers(&[(
            "accept-encoding",
            "gzip, deflate, br"
        )])));
        assert!(accepts_br(&headers(&[("accept-encoding", "BR;q=0.5")])));
        assert!(!accepts_br(&headers(&[("accept-encoding", "br;q=0")])));
        assert!(!accepts_br(&headers(&[("accept-encoding", "gzip")])));
        assert!(!accepts_br(&HeaderMap::new()));
    }

    #[test]
    fn serves_twins_etags_and_the_page() {
        let dir = std::env::temp_dir().join(format!("rune-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("snippets")).unwrap();
        std::fs::write(dir.join("index.html"), "<p>app</p>").unwrap();
        std::fs::write(dir.join("app_bg.wasm"), b"\0asm").unwrap();
        std::fs::write(dir.join("app_bg.wasm.br"), b"tiny").unwrap();
        let src = RuneSource::Dir(dir.clone());
        let ty = |r: &Response<Body>| {
            r.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let plain = src.respond("/app_bg.wasm", &HeaderMap::new());
        assert_eq!(ty(&plain).as_deref(), Some("application/wasm"));
        assert!(plain.headers().get(header::CONTENT_ENCODING).is_none());
        let br = src.respond("/app_bg.wasm", &headers(&[("accept-encoding", "br")]));
        assert_eq!(
            br.headers()
                .get(header::CONTENT_ENCODING)
                .map(HeaderValue::as_bytes),
            Some(&b"br"[..])
        );
        let etag = br.headers().get(header::ETAG).cloned().unwrap();
        assert!(etag.to_str().unwrap().ends_with("-br\""));
        let mut again = headers(&[("accept-encoding", "br")]);
        again.insert(header::IF_NONE_MATCH, etag);
        assert_eq!(
            src.respond("/app_bg.wasm", &again).status(),
            StatusCode::NOT_MODIFIED
        );
        assert_eq!(
            src.respond("/app_bg.wasm.br", &HeaderMap::new()).status(),
            StatusCode::NOT_FOUND
        );
        let page = src.respond("/sessions/abc", &HeaderMap::new());
        assert_eq!(ty(&page).as_deref(), Some("text/html;charset=utf-8"));
        assert_eq!(
            page.headers()
                .get(header::CACHE_CONTROL)
                .map(HeaderValue::as_bytes),
            Some(&b"no-store"[..])
        );
        assert_eq!(
            RuneSource::Missing.respond("/", &HeaderMap::new()).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
