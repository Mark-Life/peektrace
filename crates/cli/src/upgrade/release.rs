//! Release resolution for `upgrade` and the startup update notice. Mirrors
//! `scripts/install.sh`: the same env overrides, asset names, newest-tag
//! rule and `SHA256SUMS` format. The pure helpers sit apart from the HTTP.

use std::time::Duration;

use peektrace_core::js::value::JsValue;

use crate::ls::UserError;

/// Build-time default of a release setting, so a build can point the
/// updater at other releases with no code change.
macro_rules! build_default {
    ($var:literal, $default:literal) => {
        match option_env!($var) {
            Some(v) => v,
            None => $default,
        }
    };
}

const DEFAULT_BASE_URL: &str = build_default!(
    "PEEKTRACE_RELEASE_BASE_URL",
    "https://github.com/Mark-Life/peektrace/releases/download"
);
const DEFAULT_GITHUB_API: &str = build_default!(
    "PEEKTRACE_RELEASE_API",
    "https://api.github.com/repos/Mark-Life/peektrace"
);
/// Prefix of the release tags this binary upgrades to.
pub const TAG_PREFIX: &str = build_default!("PEEKTRACE_RELEASE_TAG_PREFIX", "cli-v");
const ASSET_PREFIX: &str = build_default!("PEEKTRACE_RELEASE_ASSET_PREFIX", "peektrace-");
const RELEASES_PER_PAGE: u32 = 100;
const USER_AGENT: &str = "peektrace-cli";
const GITHUB_ACCEPT: &str = "application/vnd.github+json";

/// The platforms with a prebuilt binary, as the error names them.
pub const SUPPORTED_PLATFORMS: &str =
    "macOS (arm64, x64), Linux (x64, arm64 — glibc), Windows (x64)";

/// Release endpoints and an optional pinned tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseConfig {
    /// Asset URLs are `<base_url>/<tag>/<asset>`.
    pub base_url: String,
    /// Releases are listed at `<github_api>/releases`.
    pub github_api: String,
    /// `PEEKTRACE_VERSION`, when set and not empty.
    pub pinned_version: Option<String>,
}

impl ReleaseConfig {
    /// From the environment; an empty value counts as unset.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        let set = |k: &str| var(k).filter(|v| !v.is_empty());
        Self {
            base_url: set("PEEKTRACE_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            github_api: set("PEEKTRACE_GITHUB_API")
                .unwrap_or_else(|| DEFAULT_GITHUB_API.to_owned()),
            pinned_version: set("PEEKTRACE_VERSION"),
        }
    }

    pub fn asset_url(&self, tag: &str, asset: &str) -> String {
        format!("{}/{tag}/{asset}", self.base_url)
    }

    pub fn checksums_url(&self, tag: &str) -> String {
        format!("{}/{tag}/SHA256SUMS", self.base_url)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Darwin,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arch {
    Arm64,
    X64,
}

/// Every prebuilt binary: host, arch and the asset suffix after the prefix.
const ASSETS: [(Os, Arch, &str); 5] = [
    (Os::Darwin, Arch::Arm64, "darwin-arm64"),
    (Os::Darwin, Arch::X64, "darwin-x64"),
    (Os::Linux, Arch::X64, "linux-x64"),
    (Os::Linux, Arch::Arm64, "linux-arm64"),
    (Os::Windows, Arch::X64, "windows-x64.exe"),
];

/// A host's release asset, or why it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Supported { asset: String, os: Os },
    Unsupported(String),
}

fn arch_of(arch: &str) -> Option<Arch> {
    match arch {
        "arm64" | "aarch64" => Some(Arch::Arm64),
        "x64" | "x86_64" | "amd64" => Some(Arch::X64),
        _ => None,
    }
}

/// The asset for a platform and arch, in Node, uname or Rust spelling.
pub fn detect_asset(platform: &str, arch: &str) -> Detection {
    let os = match platform {
        "darwin" | "Darwin" | "macos" => Os::Darwin,
        "linux" | "Linux" => Os::Linux,
        "win32" | "windows" => Os::Windows,
        _ => {
            return Detection::Unsupported(format!("unsupported operating system: {platform}"));
        }
    };
    let found = arch_of(arch).and_then(|a| {
        ASSETS
            .iter()
            .find(|(o, x, _)| *o == os && *x == a)
            .map(|(_, _, suffix)| format!("{ASSET_PREFIX}{suffix}"))
    });
    match found {
        Some(asset) => Detection::Supported { asset, os },
        None => Detection::Unsupported(match os {
            Os::Darwin => format!("unsupported macOS architecture: {arch}"),
            Os::Linux => format!(
                "unsupported Linux architecture: {arch} (prebuilt binaries exist for x64 and arm64)"
            ),
            Os::Windows => format!("unsupported Windows architecture: {arch}"),
        }),
    }
}

/// The asset for the machine this runs on.
pub fn host_asset() -> Detection {
    detect_asset(std::env::consts::OS, std::env::consts::ARCH)
}

/// The first release whose `tag_name` starts with the tag prefix. The API
/// lists releases newest first.
pub fn parse_latest_tag(releases: &JsValue) -> Option<String> {
    releases.as_arr()?.iter().find_map(|r| {
        r.get("tag_name")
            .and_then(JsValue::as_str)
            .filter(|t| t.starts_with(TAG_PREFIX))
            .map(str::to_owned)
    })
}

/// The lowercase hex digest `SHA256SUMS` lists for `asset`.
pub fn parse_checksum(sums: &str, asset: &str) -> Option<String> {
    sums.split('\n').find_map(|line| {
        let mut fields = line.split_whitespace();
        let hex = fields.next()?;
        let name = fields.last().unwrap_or(hex);
        (name == asset).then(|| hex.to_lowercase())
    })
}

/// `parseInt(s, 10)`, with no digits read as 0.
fn leading_int(s: &str) -> f64 {
    let s = s.trim_start();
    let (sign, digits) = match s.as_bytes().first() {
        Some(b'-') => (-1.0, &s[1..]),
        Some(b'+') => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    let end = digits
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse::<f64>().map_or(0.0, |n| sign * n)
}

/// The version without the tag prefix or a leading `v`.
fn version_core(v: &str) -> &str {
    v.strip_prefix(TAG_PREFIX)
        .or_else(|| v.strip_prefix('v'))
        .unwrap_or(v)
}

/// Compare dotted numeric versions, ignoring the tag prefix; a suffix such
/// as `-dev` sorts as its numeric core.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| {
        version_core(v)
            .split('.')
            .map(leading_int)
            .collect::<Vec<_>>()
    };
    let (pa, pb) = (parts(a), parts(b));
    (0..pa.len().max(pb.len()))
        .map(|i| {
            let x = pa.get(i).copied().unwrap_or(0.0);
            let y = pb.get(i).copied().unwrap_or(0.0);
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        })
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// Any accepted version spelling as a release tag.
pub fn normalize_tag(version: &str) -> String {
    if version.starts_with(TAG_PREFIX) {
        version.to_owned()
    } else {
        format!(
            "{TAG_PREFIX}{}",
            version.strip_prefix('v').unwrap_or(version)
        )
    }
}

/// An HTTP client that reads non-2xx answers as answers.
pub fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(timeout)
        .build()
        .new_agent()
}

/// Why a URL cannot be fetched by this build, when it is `https` and TLS is
/// not compiled in.
pub fn tls_missing(url: &str) -> Option<String> {
    (!cfg!(feature = "https") && url.starts_with("https:")).then(|| {
        "this peektrace build has no TLS support (build with `--features https`)".to_owned()
    })
}

/// A failed fetch: a non-2xx status or another cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Status(u16),
    Other(String),
}

/// GET `url` as bytes, with no size cap.
pub fn get_bytes(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, FetchError> {
    if let Some(m) = tls_missing(url) {
        return Err(FetchError::Other(m));
    }
    let req = headers
        .iter()
        .fold(agent.get(url), |r, (k, v)| r.header(*k, *v));
    let mut res = req.call().map_err(|e| FetchError::Other(e.to_string()))?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(FetchError::Status(status));
    }
    res.body_mut()
        .with_config()
        .limit(u64::MAX)
        .read_to_vec()
        .map_err(|e| FetchError::Other(e.to_string()))
}

/// The newest release tag from the API, `None` when it lists none.
pub fn fetch_latest_tag(
    agent: &ureq::Agent,
    config: &ReleaseConfig,
) -> Result<Option<String>, UserError> {
    let url = format!(
        "{}/releases?per_page={RELEASES_PER_PAGE}",
        config.github_api
    );
    let fail =
        |cause: String| UserError::Invalid(format!("Failed to query GitHub releases: {cause}"));
    let body = get_bytes(
        agent,
        &url,
        &[("User-Agent", USER_AGENT), ("Accept", GITHUB_ACCEPT)],
    )
    .map_err(|e| match e {
        FetchError::Status(n) => fail(format!("Error: GitHub API responded {n}")),
        FetchError::Other(m) => fail(m),
    })?;
    let text = String::from_utf8_lossy(&body);
    let releases =
        JsValue::parse(&text).ok_or_else(|| fail("SyntaxError: JSON Parse error".to_owned()))?;
    Ok(parse_latest_tag(&releases))
}

/// The tag to install: the flag pin, else `PEEKTRACE_VERSION`, else the
/// newest release.
pub fn resolve_target_tag(
    agent: &ureq::Agent,
    config: &ReleaseConfig,
    pinned: Option<&str>,
) -> Result<String, UserError> {
    if let Some(pin) = pinned.or(config.pinned_version.as_deref()) {
        return Ok(normalize_tag(pin));
    }
    fetch_latest_tag(agent, config)?.ok_or_else(|| {
        UserError::Invalid(format!(
            "Could not find a {TAG_PREFIX}* release to upgrade to."
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering::{Equal, Greater, Less};

    use super::*;

    #[test]
    fn config_from_env() {
        let c = ReleaseConfig::from_env(|k| match k {
            "PEEKTRACE_BASE_URL" | "PEEKTRACE_VERSION" => Some(String::new()),
            "PEEKTRACE_GITHUB_API" => Some("http://api".to_owned()),
            _ => None,
        });
        assert_eq!(c.base_url, DEFAULT_BASE_URL);
        assert_eq!(c.github_api, "http://api");
        assert_eq!(c.pinned_version, None);
        assert_eq!(c.asset_url("t", "a"), format!("{DEFAULT_BASE_URL}/t/a"));
    }

    /// TS `upgrade.test.ts` `detectAsset`.
    #[test]
    fn assets() {
        let ok = |p, a| match detect_asset(p, a) {
            Detection::Supported { asset, .. } => asset,
            Detection::Unsupported(r) => r,
        };
        assert_eq!(ok("darwin", "arm64"), "peektrace-darwin-arm64");
        assert_eq!(ok("Darwin", "x86_64"), "peektrace-darwin-x64");
        assert_eq!(ok("macos", "aarch64"), "peektrace-darwin-arm64");
        assert_eq!(ok("linux", "amd64"), "peektrace-linux-x64");
        assert_eq!(ok("Linux", "aarch64"), "peektrace-linux-arm64");
        assert_eq!(ok("win32", "x64"), "peektrace-windows-x64.exe");
        assert_eq!(ok("darwin", "ppc"), "unsupported macOS architecture: ppc");
        assert_eq!(
            ok("linux", "riscv64"),
            "unsupported Linux architecture: riscv64 (prebuilt binaries exist for x64 and arm64)"
        );
        assert_eq!(
            ok("windows", "arm64"),
            "unsupported Windows architecture: arm64"
        );
        assert_eq!(
            ok("freebsd", "x64"),
            "unsupported operating system: freebsd"
        );
        assert!(matches!(
            detect_asset("win32", "x64"),
            Detection::Supported {
                os: Os::Windows,
                ..
            }
        ));
    }

    /// TS `upgrade.test.ts` `parseLatestCliTag`.
    #[test]
    fn latest_tags() {
        let p = |s: &str| JsValue::parse(s).as_ref().and_then(parse_latest_tag);
        assert_eq!(
            p(
                r#"[{"tag_name":"desktop-v3.0.0"},{"tag_name":"cli-v9.9.9"},{"tag_name":"cli-v0.0.1"}]"#
            ),
            Some("cli-v9.9.9".to_owned())
        );
        assert_eq!(p(r#"[{"tag_name":"desktop-v1"}]"#), None);
        assert_eq!(p(r#"{"message":"x"}"#), None);
        assert_eq!(
            p(r#"[{"tag_name":5},{"x":1},null,{"tag_name":"cli-v1"}]"#),
            Some("cli-v1".to_owned())
        );
    }

    /// TS `upgrade.test.ts` `parseChecksum`.
    #[test]
    fn checksums() {
        let sums = "ABC123  peektrace-darwin-arm64\n\n  def  *other\nfff peektrace-linux-x64  \n";
        assert_eq!(
            parse_checksum(sums, "peektrace-darwin-arm64"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            parse_checksum(sums, "peektrace-linux-x64"),
            Some("fff".to_owned())
        );
        assert_eq!(parse_checksum(sums, "missing"), None);
        assert_eq!(parse_checksum("solo", "solo"), Some("solo".to_owned()));
    }

    /// TS `upgrade.test.ts` `compareVersions`.
    #[test]
    fn versions() {
        assert_eq!(compare_versions("cli-v9.9.9", "0.0.0-dev"), Greater);
        assert_eq!(compare_versions("cli-v0.0.0", "0.0.0-dev"), Equal);
        assert_eq!(compare_versions("v1.2", "1.2.0"), Equal);
        assert_eq!(compare_versions("1.10.0", "1.9.9"), Greater);
        assert_eq!(compare_versions("cli-v0.11.0", "cli-v0.12.0"), Less);
        assert_eq!(compare_versions("x", "0"), Equal);
        assert_eq!(normalize_tag("1.2.3"), "cli-v1.2.3");
        assert_eq!(normalize_tag("v1.2.3"), "cli-v1.2.3");
        assert_eq!(normalize_tag("cli-v1.2.3"), "cli-v1.2.3");
        assert_eq!(normalize_tag("0.0.0-dev"), "cli-v0.0.0-dev");
    }

    /// TS `upgrade.test.ts` "pure helpers", with its own fixtures.
    #[test]
    fn pure_helpers_ts_fixtures() {
        let p = |s: &str| JsValue::parse(s).as_ref().and_then(parse_latest_tag);
        assert_eq!(
            p(
                r#"[{"tag_name":"desktop-v3.0.0"},{"tag_name":"cli-v2.1.0"},{"tag_name":"cli-v1.0.0"}]"#
            ),
            Some("cli-v2.1.0".to_owned())
        );
        assert_eq!(p(r#"[{"tag_name":"desktop-v1.0.0"}]"#), None);
        assert_eq!(p(r#""nope""#), None);

        let sums = "abc123  peektrace-linux-x64\ndef456  peektrace-darwin-arm64\n";
        assert_eq!(
            parse_checksum(sums, "peektrace-darwin-arm64"),
            Some("def456".to_owned())
        );
        assert_eq!(parse_checksum(sums, "peektrace-windows-x64.exe"), None);

        let asset = |p, a| match detect_asset(p, a) {
            Detection::Supported { asset, .. } => Some(asset),
            Detection::Unsupported(_) => None,
        };
        assert_eq!(
            asset("darwin", "arm64").as_deref(),
            Some("peektrace-darwin-arm64")
        );
        assert_eq!(asset("linux", "x64").as_deref(), Some("peektrace-linux-x64"));
        assert_eq!(
            asset("Linux", "aarch64").as_deref(),
            Some("peektrace-linux-arm64")
        );
        assert!(matches!(
            detect_asset("win32", "x64"),
            Detection::Supported { ref asset, os: Os::Windows } if asset == "peektrace-windows-x64.exe"
        ));
        assert!(matches!(
            detect_asset("freebsd", "x64"),
            Detection::Unsupported(_)
        ));

        assert_eq!(compare_versions("cli-v9.9.9", "0.0.1"), Greater);
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Equal);
        assert_eq!(compare_versions("v1.0.0", "v1.2.0"), Less);
        assert_eq!(compare_versions("0.0.0-dev", "cli-v0.0.1"), Less);
    }

    #[test]
    fn leading_ints() {
        assert!((leading_int("0-dev") - 0.0).abs() < f64::EPSILON);
        assert!((leading_int("12abc") - 12.0).abs() < f64::EPSILON);
        assert!((leading_int(" -3") + 3.0).abs() < f64::EPSILON);
    }
}
