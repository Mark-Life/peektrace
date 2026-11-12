//! Token auth for non-loopback binds. A bind to any interface other than
//! loopback needs a 256-bit token, held as a `Bearer` header, as an
//! `HttpOnly` cookie, or once as `?token=`, which sets the cookie and
//! redirects the token out of the URL.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use peektrace_core::js::{text, units};
use sha2::{Digest as _, Sha256};

/// Env var that supplies the token.
pub const TOKEN_ENV: &str = "PEEKTRACE_TOKEN";
/// Shortest token accepted from [`TOKEN_ENV`], in UTF-16 units.
const MIN_TOKEN_LENGTH: usize = 32;
const TOKEN_BYTES: usize = 32;
const QUERY_PARAM: &str = "token";

/// True when `host` is a loopback interface.
pub fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Auth and write posture from the bind host and flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posture {
    pub auth: bool,
    pub read_only: bool,
}

/// Non-loopback binds need a token and are read-only unless `allow_write`;
/// `read_only` always wins.
pub fn posture(host: &str, read_only: bool, allow_write: bool) -> Posture {
    let auth = !is_loopback(host);
    Posture {
        auth,
        read_only: read_only || (auth && !allow_write),
    }
}

/// Random 256-bit token, base64url without padding.
pub fn make_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The token to enforce: [`TOKEN_ENV`] trimmed when set and long enough,
/// else a random one. `Err` holds the message for the user.
pub fn resolve_token(env_value: Option<&str>) -> Result<String, String> {
    let from_env = env_value.map(text::trim).filter(|t| !t.is_empty());
    match from_env {
        None => make_token().map_err(|e| format!("Cannot make a token: {e}")),
        Some(t) if units::len(t) < MIN_TOKEN_LENGTH => Err(format!(
            "{TOKEN_ENV} must be at least {MIN_TOKEN_LENGTH} characters. Unset it to get a random token."
        )),
        Some(t) => Ok(t.to_owned()),
    }
}

/// Constant-time equality over the SHA-256 digests, so length never leaks.
pub fn tokens_equal(a: &str, b: &str) -> bool {
    let (da, db) = (Sha256::digest(a), Sha256::digest(b));
    da.iter()
        .zip(db.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Cookies are shared across ports on one host, so the port is in the name.
pub fn cookie_name(port: u16) -> String {
    format!("peektrace_token_{port}")
}

/// `Set-Cookie` value for a redirect that hands over the token.
pub fn set_cookie(port: u16, token: &str) -> String {
    format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax",
        cookie_name(port)
    )
}

/// What `encodeURIComponent` escapes.
const URI_COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// `encodeURIComponent(s)`.
pub fn encode_uri_component(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, URI_COMPONENT).to_string()
}

/// URL a person opens: the base URL with the token as a query param.
pub fn token_url(base: &str, token: &str) -> String {
    format!("{base}/?{QUERY_PARAM}={}", encode_uri_component(token))
}

/// The first cookie called `name`: quotes stripped, percent-decoded.
pub fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k.trim() != name {
            return None;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(v);
        Some(
            percent_encoding::percent_decode_str(v)
                .decode_utf8()
                .map_or_else(|_| v.to_owned(), std::borrow::Cow::into_owned),
        )
    })
}

/// The token of an `Authorization: Bearer <t>` header.
pub fn bearer_of(header: &str) -> Option<&str> {
    let rest = header
        .get(..6)
        .filter(|p| p.eq_ignore_ascii_case("bearer"))?;
    let after = &header[rest.len()..];
    let trimmed = after.trim_start_matches(|c: char| c.is_whitespace());
    (trimmed.len() < after.len()).then_some(trimmed)
}

/// The slice of a request the auth decision reads.
#[derive(Debug, Clone, Copy)]
pub struct AuthRequest<'a> {
    pub method: &'a str,
    /// Path and query as sent.
    pub target: &'a str,
    pub authorization: Option<&'a str>,
    pub cookie: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Ok,
    Unauthorized,
    Redirect(String),
}

/// Same path and query, minus every `token` param. A leading `//` would
/// make the redirect protocol-relative, so it collapses to `/`.
fn strip_token(url: &url::Url) -> String {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(url.query_pairs().filter(|(k, _)| k != QUERY_PARAM))
        .finish();
    let path = url.path();
    let rest = path.trim_start_matches('/');
    let path = if path.len() - rest.len() >= 2 {
        format!("/{rest}")
    } else {
        path.to_owned()
    };
    if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    }
}

/// Decide a request: Bearer, then cookie, then `?token=` on GET. A GET with
/// `?token=` and a valid token by any means redirects.
pub fn auth_outcome(req: AuthRequest<'_>, token: &str, port: u16) -> AuthOutcome {
    let bearer = req.authorization.and_then(bearer_of);
    let cookie = req.cookie.and_then(|c| cookie_value(c, &cookie_name(port)));
    let held = bearer.is_some_and(|b| tokens_equal(b, token))
        || cookie.is_some_and(|c| tokens_equal(&c, token));
    if req.method == "GET" {
        let url = url::Url::parse("http://peektrace.invalid")
            .and_then(|base| base.join(req.target))
            .ok();
        if let Some(url) = url {
            let query_token = url
                .query_pairs()
                .find(|(k, _)| k == QUERY_PARAM)
                .map(|(_, v)| v.into_owned());
            if query_token.is_some_and(|q| held || tokens_equal(&q, token)) {
                return AuthOutcome::Redirect(strip_token(&url));
            }
        }
    }
    if held {
        AuthOutcome::Ok
    } else {
        AuthOutcome::Unauthorized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

    fn get(target: &str) -> AuthRequest<'_> {
        AuthRequest {
            method: "GET",
            target,
            authorization: None,
            cookie: None,
        }
    }

    #[test]
    fn token_shape() {
        let t = make_token().unwrap();
        assert_eq!(t.len(), 43);
        assert!(
            t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(t, make_token().unwrap());
    }

    #[test]
    fn equality() {
        assert!(tokens_equal(TOKEN, TOKEN));
        assert!(!tokens_equal(TOKEN, "x"));
        assert!(!tokens_equal("", TOKEN));
    }

    #[test]
    fn posture_table() {
        assert_eq!(
            posture("127.0.0.1", false, false),
            Posture {
                auth: false,
                read_only: false
            }
        );
        assert_eq!(
            posture("localhost", true, false),
            Posture {
                auth: false,
                read_only: true
            }
        );
        assert_eq!(
            posture("0.0.0.0", false, false),
            Posture {
                auth: true,
                read_only: true
            }
        );
        assert_eq!(
            posture("0.0.0.0", false, true),
            Posture {
                auth: true,
                read_only: false
            }
        );
        assert_eq!(
            posture("::", true, true),
            Posture {
                auth: true,
                read_only: true
            }
        );
        assert!(posture("LOCALHOST", false, false).auth);
    }

    #[test]
    fn env_token() {
        assert_eq!(
            resolve_token(Some(&format!("{TOKEN}\r"))),
            Ok(TOKEN.to_owned())
        );
        assert_eq!(resolve_token(Some(" ")).map(|t| t.len()), Ok(43));
        assert!(
            resolve_token(Some("short"))
                .unwrap_err()
                .starts_with("PEEKTRACE_TOKEN must be")
        );
        assert_eq!(resolve_token(None).map(|t| t.len()), Ok(43));
    }

    #[test]
    fn redirect_strips_only_token() {
        let t = format!("/a%20b/?x=a+b&token={TOKEN}&y=%41");
        assert_eq!(
            auth_outcome(get(&t), TOKEN, 1),
            AuthOutcome::Redirect("/a%20b/?x=a+b&y=A".to_owned())
        );
        let t = format!("//evil.com?token={TOKEN}");
        assert_eq!(
            auth_outcome(get(&t), TOKEN, 1),
            AuthOutcome::Redirect("/".to_owned())
        );
        let t = format!("/.//evil.example/x?token={TOKEN}");
        assert_eq!(
            auth_outcome(get(&t), TOKEN, 1),
            AuthOutcome::Redirect("/evil.example/x".to_owned())
        );
        let t = format!("/?token={TOKEN}&token=bad");
        assert!(matches!(
            auth_outcome(get(&t), TOKEN, 1),
            AuthOutcome::Redirect(_)
        ));
        let t = format!("/?token=bad&token={TOKEN}");
        assert_eq!(auth_outcome(get(&t), TOKEN, 1), AuthOutcome::Unauthorized);
    }

    #[test]
    fn no_redirect_on_non_get() {
        let t = format!("/?token={TOKEN}");
        let req = AuthRequest {
            method: "HEAD",
            ..get(&t)
        };
        assert_eq!(auth_outcome(req, TOKEN, 1), AuthOutcome::Unauthorized);
    }

    #[test]
    fn credentials() {
        let bearer = format!("bearer   {TOKEN}");
        let req = AuthRequest {
            authorization: Some(&bearer),
            ..get("/")
        };
        assert_eq!(auth_outcome(req, TOKEN, 1), AuthOutcome::Ok);
        let basic = format!("Basic {TOKEN}");
        let req = AuthRequest {
            authorization: Some(&basic),
            ..get("/")
        };
        assert_eq!(auth_outcome(req, TOKEN, 1), AuthOutcome::Unauthorized);
        let cookies = format!("peektrace_token_7=bad; peektrace_token_7={TOKEN}");
        let req = AuthRequest {
            cookie: Some(&cookies),
            ..get("/")
        };
        assert_eq!(auth_outcome(req, TOKEN, 7), AuthOutcome::Unauthorized);
        let cookies = format!("a=1; peektrace_token_7=\"{TOKEN}\"");
        let req = AuthRequest {
            cookie: Some(&cookies),
            ..get("/")
        };
        assert_eq!(auth_outcome(req, TOKEN, 7), AuthOutcome::Ok);
        let encoded = format!("peektrace_token_7=%61{}", &TOKEN[1..]);
        let req = AuthRequest {
            cookie: Some(&encoded),
            ..get("/")
        };
        assert_eq!(auth_outcome(req, TOKEN, 7), AuthOutcome::Ok);
        let t = format!("/x?token={TOKEN}");
        let cookie = format!("peektrace_token_7={TOKEN}");
        let req = AuthRequest {
            cookie: Some(&cookie),
            ..get(&t)
        };
        assert_eq!(
            auth_outcome(req, TOKEN, 7),
            AuthOutcome::Redirect("/x".to_owned())
        );
    }

    #[test]
    fn names_and_urls() {
        assert_eq!(cookie_name(4321), "peektrace_token_4321");
        assert_eq!(
            token_url("http://h:1", "a b+/~!"),
            "http://h:1/?token=a%20b%2B%2F~!"
        );
    }
}
