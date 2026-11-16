//! DNS-rebinding and cross-origin guards, from the bound host and port.

use crate::auth::is_loopback;

pub const HOST_FORBIDDEN: &str = "request rejected (Host header not allowed)";
pub const ORIGIN_FORBIDDEN: &str = "cross-origin request rejected (Origin not allowed)";

/// Host authorities and origins that may reach this server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    hosts: Vec<String>,
    origins: Vec<String>,
}

impl Allowlist {
    /// The three loopback authorities, plus the bind host when it is not
    /// loopback.
    pub fn new(host: &str, port: u16) -> Self {
        let mut hosts = vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ];
        if !is_loopback(host) {
            hosts.push(format!("{host}:{port}"));
        }
        let origins = hosts.iter().map(|h| format!("http://{h}")).collect();
        Self { hosts, origins }
    }

    /// `None` when the `Host` header passes. Under auth any present Host
    /// passes: the token is what protects the server.
    pub fn host_refusal(&self, host: Option<&str>, authed: bool) -> Option<&'static str> {
        match host {
            Some(h) if authed || self.hosts.iter().any(|a| a == h) => None,
            _ => Some(HOST_FORBIDDEN),
        }
    }

    /// `None` when the `Origin` header passes. An absent Origin passes;
    /// under auth it must name the request's own Host over http or https.
    pub fn origin_refusal(
        &self,
        origin: Option<&str>,
        host: Option<&str>,
        authed: bool,
    ) -> Option<&'static str> {
        let origin = origin?;
        let ok = if authed {
            host.is_some_and(|h| {
                origin
                    .strip_prefix("http://")
                    .or_else(|| origin.strip_prefix("https://"))
                    == Some(h)
            })
        } else {
            self.origins.iter().any(|o| o == origin)
        };
        (!ok).then_some(ORIGIN_FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts() {
        let a = Allowlist::new("127.0.0.1", 4610);
        assert_eq!(a.host_refusal(Some("127.0.0.1:4610"), false), None);
        assert_eq!(a.host_refusal(Some("[::1]:4610"), false), None);
        assert!(a.host_refusal(Some("LOCALHOST:4610"), false).is_some());
        assert!(a.host_refusal(Some("127.0.0.1"), false).is_some());
        assert!(a.host_refusal(Some("127.0.0.1:4611"), false).is_some());
        assert!(a.host_refusal(None, false).is_some());
        assert!(a.host_refusal(None, true).is_some());
        assert_eq!(a.host_refusal(Some("evil.com"), true), None);
    }

    #[test]
    fn origins() {
        let a = Allowlist::new("0.0.0.0", 4620);
        assert_eq!(a.origin_refusal(None, None, false), None);
        assert_eq!(
            a.origin_refusal(Some("http://0.0.0.0:4620"), None, false),
            None
        );
        assert!(
            a.origin_refusal(Some("https://127.0.0.1:4620"), None, false)
                .is_some()
        );
        let host = Some("127.0.0.1:4620");
        assert_eq!(
            a.origin_refusal(Some("https://127.0.0.1:4620"), host, true),
            None
        );
        assert!(
            a.origin_refusal(Some("http://evil.com"), host, true)
                .is_some()
        );
    }
}
