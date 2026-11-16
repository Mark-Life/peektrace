//! Which missing binaries the report's host has now, so the
//! command-not-found fix stops naming what is already installed.

use tracing::info_span;

use crate::js::regex::LazyRe;
use crate::js_re;
use crate::sessions::resolve::exists;

/// Dirs on every login PATH: Debian/Ubuntu, Fedora, macOS and Homebrew's.
const SYSTEM_BIN_DIRS: [&str; 7] = [
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/opt/homebrew/bin",
];
const CHECK_CAP: usize = 32;
static PLAIN_NAME: LazyRe = js_re!(r"^[\w.+-]+$");

/// The names of `bins` that a system bin dir holds.
pub fn host_bins_of(bins: &[String]) -> Vec<String> {
    let _span = info_span!("Stats.hostBins").entered();
    bins.iter()
        .filter(|b| PLAIN_NAME.is_match(b))
        .take(CHECK_CAP)
        .filter(|b| SYSTEM_BIN_DIRS.iter().any(|d| exists(&format!("{d}/{b}"))))
        .cloned()
        .collect()
}
