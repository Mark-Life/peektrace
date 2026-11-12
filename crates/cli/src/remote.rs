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
        if !self.url.contains("://") {
            return Err(self.fail(&Transport::Detail("ERR_INVALID_URL".to_owned())));
        }
        if !is_http_url(&self.url) {
            return Err(self.fail(&Transport::Detail("ERR_INVALID_URL".to_owned())));
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
        let mut res = req.call().map_err(|e| self.fail(&transport_detail(&e)))?;
        let status = res.status().as_u16();
        let is_json = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        let body = res.body_mut().read_to_string().ok();
        let value = body.as_deref().filter(|_| is_json).and_then(JsValue::parse);
        match (status, value) {
            (200..=299, Some(v)) => Ok(v),
            (200..=299, None) => {
                Err(self.fail(&Transport::Detail("unexpected response".to_owned())))
            }
            (_, Some(v)) if v.get("error").is_some() => Err(UserError::Invalid(format_api_error(
                v.get("error").unwrap_or(&v),
            ))),
            (n, _) => Err(self.fail(&Transport::Status(n))),
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
        let e = JsValue::parse(r#"{"_tag":"NotPortedYetError","feature":"stats"}"#).unwrap();
        assert_eq!(format_api_error(&e), "Error: NotPortedYetError");
    }
}
