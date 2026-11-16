//! `--remote <url>`: run session commands against a `peektrace serve`.

use std::io::ErrorKind;

use peektrace_core::js::text;
use peektrace_core::js::value::JsValue;
use peektrace_server::auth::encode_uri_component;

use crate::ls::UserError;

const QUERY_PARAM: &str = "token";

/// True for a parseable `http:` or `https:` URL.
fn is_http_url(s: &str) -> bool {
    url::Url::parse(s).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

/// Mask every `token=` value of a text that is not an http(s) URL.
fn mask_token(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("token=") {
        let marked = at > 0 && matches!(rest.as_bytes()[at - 1], b'?' | b'&');
        let value_at = at + "token=".len();
        out.push_str(&rest[..value_at]);
        rest = &rest[value_at..];
        if marked {
            let end = rest.find(['&', '#']).unwrap_or(rest.len());
            out.push_str("***");
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

/// A `--remote` value split into the URL to show and call, and the token it
/// carries. An empty token counts as none; the token never stays in the URL.
pub fn split_remote(remote: &str) -> (String, Option<String>) {
    if !is_http_url(remote) {
        return (mask_token(remote), None);
    }
    let Ok(parsed) = url::Url::parse(remote) else {
        return (remote.to_owned(), None);
    };
    let token = parsed
        .query_pairs()
        .find(|(k, _)| k == QUERY_PARAM)
        .map(|(_, v)| v.into_owned());
    match token {
        Some(t) => (
            parsed.origin().ascii_serialization(),
            Some(t).filter(|t| !t.is_empty()),
        ),
        None => (remote.to_owned(), None),
    }
}

/// A server to call: its URL as typed (token removed) and the bearer token.
pub struct Remote {
    url: String,
    bearer: Option<String>,
    agent: ureq::Agent,
}

/// A failed call: the server error's `_tag`, when it sent one, and the CLI
/// message.
#[derive(Debug)]
pub struct Failure {
    pub tag: Option<String>,
    pub error: UserError,
}

/// A failed fetch: a server error body, or any other failure.
enum Fetch {
    Api(JsValue),
    Other(UserError),
}

/// How a call failed below the API: an HTTP status or a transport detail.
enum Transport {
    Status(u16),
    Detail(String),
}

impl Remote {
    /// A URL token wins over `env_token` (`PEEKTRACE_TOKEN`).
    pub fn new(remote: &str, env_token: Option<&str>) -> Self {
        let (url, token) = split_remote(remote);
        let bearer = token.or_else(|| {
            env_token
                .map(text::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        });
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self { url, bearer, agent }
    }

    fn transport_message(&self, t: &Transport) -> String {
        let url = &self.url;
        if text::trim(url).is_empty() {
            return "Invalid --remote URL: it is empty. Use `http://host:port`.".to_owned();
        }
        match t {
            Transport::Status(401) => {
                return format!(
                    "Remote request to {url} was unauthorized (HTTP 401). Set PEEKTRACE_TOKEN or pass the printed ?token= URL."
                );
            }
            Transport::Status(403) => {
                return format!(
                    "Remote request to {url} was refused (HTTP 403). Check that the local and remote ports match when using ssh -L (the Host header must be the server's own address)."
                );
            }
            _ => {}
        }
        let detail = match t {
            Transport::Status(n) => format!("HTTP {n}"),
            Transport::Detail(d) => d.clone(),
        };
        if !url.contains("://") {
            return format!(
                "Remote request to {url} failed ({detail}). Include the scheme, for example `http://host:port`."
            );
        }
        if !is_http_url(url) {
            return format!("Invalid --remote URL {url}. Use `http://host:port`.");
        }
        format!(
            "Remote request to {url} failed ({detail}). Check that `peektrace serve` is running there."
        )
    }

    /// A 2xx answer that is not the expected JSON.
    pub fn unexpected(&self) -> UserError {
        self.fail(&Transport::Detail("unexpected response".to_owned()))
    }

    fn fail(&self, t: &Transport) -> UserError {
        UserError::Invalid(self.transport_message(t))
    }

    /// `GET <url>/api/<segments...>?<query>` as JSON. A server error body
    /// becomes its CLI message.
    pub fn get_json(
        &self,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> Result<JsValue, UserError> {
        self.get(segments, query).map_err(|f| f.error)
    }

    /// [`Self::get_json`], keeping the `_tag` of a server error.
    pub fn get(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<JsValue, Failure> {
        self.fetch(segments, query).map_err(|e| match e {
            Fetch::Api(v) => {
                let body = v.get("error").unwrap_or(&v);
                Failure {
                    tag: body
                        .get("_tag")
                        .and_then(JsValue::as_str)
                        .map(str::to_owned),
                    error: UserError::Invalid(format_api_error(body)),
                }
            }
            Fetch::Other(error) => Failure { tag: None, error },
        })
    }

    fn fetch(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<JsValue, Fetch> {
        if !self.url.contains("://") {
            return Err(Fetch::Other(
                self.fail(&Transport::Detail("ERR_INVALID_URL".to_owned())),
            ));
        }
        if !is_http_url(&self.url) {
            return Err(Fetch::Other(
                self.fail(&Transport::Detail("ERR_INVALID_URL".to_owned())),
            ));
        }
        if !cfg!(feature = "https") && self.url.starts_with("https:") {
            return Err(Fetch::Other(UserError::Invalid(format!(
                "Cannot reach {}: this peektrace build has no TLS support. Use an http:// URL (for example through ssh -L), or build with `--features https`.",
                self.url
            ))));
        }
        let base = self.url.strip_suffix('/').unwrap_or(&self.url);
        let path = segments.iter().fold(String::new(), |mut acc, s| {
            acc.push('/');
            acc.push_str(&encode_uri_component(s));
            acc
        });
        let mut target = format!("{base}/api{path}");
        if !query.is_empty() {
            let q = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(query)
                .finish();
            target = format!("{target}?{q}");
        }
        let mut req = self.agent.get(&target).header("Accept", "application/json");
        if let Some(t) = &self.bearer {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        let mut res = req
            .call()
            .map_err(|e| Fetch::Other(self.fail(&transport_detail(&e))))?;
        let status = res.status().as_u16();
        let is_json = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        // ureq caps `read_to_string` at 10 MB; an export can be far larger.
        let body = res
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .read_to_string()
            .map_err(|e| {
                Fetch::Other(self.fail(&Transport::Detail(format!("reading the response: {e}"))))
            })?;
        let value = is_json.then_some(body.as_str()).and_then(JsValue::parse);
        match (status, value) {
            (200..=299, Some(v)) => Ok(v),
            (200..=299, None) => Err(Fetch::Other(
                self.fail(&Transport::Detail("unexpected response".to_owned())),
            )),
            (_, Some(v)) if v.get("error").is_some() => Err(Fetch::Api(v)),
            (n, _) => Err(Fetch::Other(self.fail(&Transport::Status(n)))),
        }
    }
}

fn transport_detail(e: &ureq::Error) -> Transport {
    let code = match e {
        ureq::Error::StatusCode(n) => return Transport::Status(*n),
        ureq::Error::HostNotFound => "ENOTFOUND",
        ureq::Error::Timeout(_) => "ETIMEDOUT",
        ureq::Error::BadUri(_) => "ERR_INVALID_URL",
        ureq::Error::Io(io) => match io.kind() {
            ErrorKind::ConnectionRefused => "ECONNREFUSED",
            ErrorKind::TimedOut => "ETIMEDOUT",
            ErrorKind::NotFound => "ENOTFOUND",
            ErrorKind::ConnectionReset => "ECONNRESET",
            _ => return Transport::Detail(io.to_string()),
        },
        other => return Transport::Detail(other.to_string()),
    };
    Transport::Detail(code.to_owned())
}

/// `formatCliError` for an API error object.
fn format_api_error(e: &JsValue) -> String {
    let f = |k: &str| peektrace_core::js::value::js_string(e.get(k));
    match e.get("_tag").and_then(JsValue::as_str).unwrap_or("") {
        "SessionNotFoundError" => format!(
            "Session \"{}\" not found (searched {}).",
            f("id"),
            f("searchedRoot")
        ),
        "MemoryNotFoundError" => format!(
            "Memory \"{}\" not found in project \"{}\".",
            f("name"),
            f("project")
        ),
        "MemoryValidationError" => {
            let named = e
                .get("name")
                .and_then(JsValue::as_str)
                .map(|n| format!(" \"{n}\""))
                .unwrap_or_default();
            format!("Invalid memory{named}: {}", f("reason"))
        }
        "FileChangedError" => format!(
            "File changed on disk ({}): {}. Reload and retry.",
            f("reason"),
            f("path")
        ),
        "PathOutsideRootError" => {
            format!("Refused: path escapes the allowed roots: {}", f("path"))
        }
        "CapabilityUnsupportedError" => format!(
            "Unsupported for agent \"{}\": {}",
            f("agentId"),
            f("capabilityId")
        ),
        "TranscriptParseError" => {
            format!("Failed to parse transcript {}: {}", f("path"), f("reason"))
        }
        "StatsScanError" => format!("Could not scan the corpus: {}", f("reason")),
        "StatsRowNotFoundError" => format!(
            "No {} row with key \"{}\". Run `peektrace stats` and copy a DRILL cell.",
            f("table"),
            f("key")
        ),
        tag => e
            .get("message")
            .and_then(JsValue::as_str)
            .map_or_else(|| format!("Error: {tag}"), str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        assert_eq!(
            split_remote("http://h:1/base?token=abc&x=1"),
            ("http://h:1".to_owned(), Some("abc".to_owned()))
        );
        assert_eq!(
            split_remote("http://h:1/?token="),
            ("http://h:1".to_owned(), None)
        );
        assert_eq!(
            split_remote("http://h:1/base/"),
            ("http://h:1/base/".to_owned(), None)
        );
        assert_eq!(
            split_remote("h:1/?token=secret&a=1#x"),
            ("h:1/?token=***&a=1#x".to_owned(), None)
        );
        assert_eq!(
            split_remote("ftp://x?token=s"),
            ("ftp://x?token=***".to_owned(), None)
        );
    }

    /// Serve one canned answer on a local port; returns the URL and the
    /// request head it received.
    fn one_shot(body: String) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{BufRead as _, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let head: Vec<String> = BufReader::new(conn.try_clone().expect("clone"))
                .lines()
                .map_while(Result::ok)
                .take_while(|l| !l.is_empty())
                .collect();
            let res = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            conn.write_all(res.as_bytes()).expect("write");
            head.join("\n").to_ascii_lowercase()
        });
        (url, handle)
    }

    #[test]
    fn reads_bodies_over_10_mb_and_asks_for_gzip() {
        let body = format!("{{\"x\":\"{}\"}}", "a".repeat(11 * 1024 * 1024));
        let (url, server) = one_shot(body);
        let v = Remote::new(&url, None).get_json(&["sessions"], &[]);
        let head = server.join().expect("server");
        let len = v
            .ok()
            .and_then(|v| v.get("x").and_then(JsValue::as_str).map(str::len));
        assert_eq!(len, Some(11 * 1024 * 1024));
        assert!(head.contains("accept-encoding: gzip"), "{head}");
    }

    #[cfg(not(feature = "https"))]
    #[test]
    fn https_without_tls_says_so() {
        let Err(UserError::Invalid(m)) =
            Remote::new("https://example.com", None).get_json(&["sessions"], &[])
        else {
            panic!("expected an error");
        };
        assert!(m.contains("no TLS support"), "{m}");
    }

    #[test]
    fn messages() {
        let r = Remote::new("", None);
        assert_eq!(
            r.transport_message(&Transport::Status(500)),
            "Invalid --remote URL: it is empty. Use `http://host:port`."
        );
        let r = Remote::new("ftp://x", None);
        assert_eq!(
            r.transport_message(&Transport::Detail("x".to_owned())),
            "Invalid --remote URL ftp://x. Use `http://host:port`."
        );
        let r = Remote::new("127.0.0.1:4610", None);
        assert_eq!(
            r.transport_message(&Transport::Detail("ERR_INVALID_URL".to_owned())),
            "Remote request to 127.0.0.1:4610 failed (ERR_INVALID_URL). Include the scheme, for example `http://host:port`."
        );
        let r = Remote::new("http://127.0.0.1:4620", None);
        assert!(
            r.transport_message(&Transport::Status(401))
                .contains("unauthorized (HTTP 401)")
        );
    }

    #[test]
    fn api_errors() {
        let e = JsValue::parse(
            r#"{"_tag":"SessionNotFoundError","id":"nope","searchedRoot":"all agents"}"#,
        )
        .unwrap();
        assert_eq!(
            format_api_error(&e),
            "Session \"nope\" not found (searched all agents)."
        );
        let e = JsValue::parse(r#"{"_tag":"SomeNewError","feature":"stats"}"#).unwrap();
        assert_eq!(format_api_error(&e), "Error: SomeNewError");
    }
    // serve-auth.test.ts splitRemote cases and the 403 port hint.

    #[test]
    fn split_remote_never_leaks_or_loses_the_token() {
        for raw in [
            "127.0.0.1:47812/?token=SECRET",
            "server:4321/?token=SECRET",
            "localhost:1/?token=SECRET",
        ] {
            let (url, token) = split_remote(raw);
            assert!(!url.contains("SECRET"), "{url}");
            assert_ne!(url, "null");
            assert_eq!(token, None);
        }
        assert_eq!(split_remote("http://h:1/?token=").1, None);
    }

    #[test]
    fn split_remote_separates_the_token_from_the_url() {
        let token = "t".repeat(43);
        assert_eq!(
            split_remote(&format!("http://h:1/?token={token}")),
            ("http://h:1".to_owned(), Some(token))
        );
        assert_eq!(split_remote("http://h:1"), ("http://h:1".to_owned(), None));
        assert_eq!(split_remote("h:1").1, None);
    }

    #[test]
    fn forbidden_maps_to_a_port_hint() {
        let r = Remote::new("http://127.0.0.1:4321", None);
        assert!(
            r.transport_message(&Transport::Status(403))
                .contains("ports match")
        );
    }
}
