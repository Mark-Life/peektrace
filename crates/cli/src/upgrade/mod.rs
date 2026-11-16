//! `peektrace upgrade`: resolve the target release tag, download the new
//! binary, verify its sha256, then atomically replace the installed one.
//! `--check` only reports. Windows cannot replace a running `.exe`, so it
//! points at the installer.

pub mod install;
pub mod notice;
pub mod release;
pub mod version;

use std::cmp::Ordering;
use std::path::PathBuf;

use self::release::{BUILD_FEED, Detection, Os, ReleaseConfig, SUPPORTED_PLATFORMS};
use crate::args::UpgradeArgs;
use crate::ls::UserError;
use crate::render::bytes;

/// Hidden test hook: the file `upgrade` replaces instead of the running
/// binary.
pub const TARGET_ENV: &str = "PEEKTRACE_UPGRADE_TARGET";

const WINDOWS_INSTALL_HINT: &str = "Automatic in-place upgrade is not supported on Windows (a running .exe can't replace itself).\n\
Re-run the installer to update:\n  \
irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex";

const NO_FEED: &str = "Upgrade is not available in this build: it has no release feed. \
Set PEEKTRACE_BASE_URL and PEEKTRACE_GITHUB_API to use one.";

/// What `upgrade` reads from the process.
pub struct UpgradeEnv {
    pub version: &'static str,
    /// `None` when neither the build nor the environment names a feed.
    pub config: Option<ReleaseConfig>,
    pub host: Detection,
    pub exe: String,
    /// The file to replace: the hook, else the real path of the binary.
    pub target: Option<PathBuf>,
}

impl UpgradeEnv {
    pub fn from_process(version: &'static str) -> Self {
        let var = |k: &str| std::env::var(k).ok();
        let target = var(TARGET_ENV)
            .filter(|t| !t.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().and_then(std::fs::canonicalize).ok());
        Self {
            version,
            config: ReleaseConfig::from_env(BUILD_FEED, var),
            host: release::host_asset(),
            exe: notice::exe_path(),
            target,
        }
    }
}

fn is_newer(tag: &str, installed: &str) -> bool {
    release::compare_versions(tag, installed) == Ordering::Greater
}

/// Install only a newer release, so a build ahead of the latest release is
/// never downgraded; a version the user names installs as asked.
pub fn should_install(tag: &str, installed: &str, forced: bool) -> bool {
    forced || is_newer(tag, installed)
}

/// Both versions, so a build ahead of the feed says so.
fn up_to_date_line(installed: &str, tag: &str, forced: bool) -> String {
    let which = if forced { "requested" } else { "latest" };
    format!(
        "peektrace is up to date: installed {}, {which} release {tag}.",
        release::normalize_tag(installed)
    )
}

/// Run `upgrade`; each stdout line goes to `print` as it is decided, so
/// `Fetching ...` shows before the download.
pub fn run(
    args: &UpgradeArgs,
    env: &UpgradeEnv,
    print: &mut dyn FnMut(&str),
) -> Result<(), UserError> {
    let Some(config) = &env.config else {
        return Err(UserError::Invalid(NO_FEED.to_owned()));
    };
    let (asset, os) = match &env.host {
        Detection::Supported { asset, os } => (asset.as_str(), *os),
        Detection::Unsupported(reason) => {
            return Err(UserError::Invalid(format!(
                "{reason}. Supported: {SUPPORTED_PLATFORMS}."
            )));
        }
    };
    let agent = release::agent(None);
    let pinned = args.version.as_deref();
    let tag = release::resolve_target_tag(&agent, config, pinned)?;
    let forced = pinned.is_some() || config.pinned_version.is_some();
    let up_to_date = up_to_date_line(env.version, &tag, forced);
    if args.check {
        if is_newer(&tag, env.version) {
            print(&notice::update_hint(&tag, &env.exe));
        } else {
            print(&up_to_date);
        }
        return Ok(());
    }
    if !should_install(&tag, env.version, forced) {
        print(&up_to_date);
        return Ok(());
    }
    if os == Os::Windows {
        print(WINDOWS_INSTALL_HINT);
        return Ok(());
    }
    let Some(target) = &env.target else {
        return Err(UserError::Invalid(
            "Cannot find the running peektrace binary to replace.".to_owned(),
        ));
    };
    print(&format!("Fetching {asset} ({tag})..."));
    let size = install::perform_upgrade(&agent, config, &tag, asset, target)?;
    print(&format!(
        "Upgraded to {tag} (downloaded {}). It takes effect on the next launch.",
        bytes(size as u64)
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_only_newer_unless_forced() {
        let cases = [
            ("cli-v0.13.0", "0.12.0", false, true),
            ("cli-v0.13.0", "0.13.0", false, false),
            ("cli-v0.12.0", "0.13.0", false, false),
            ("cli-v0.12.9", "0.13.0-dev", false, false),
            ("cli-v1.0.0", "0.13.0", false, true),
            ("cli-v0.13.0", "0.13.0-rc.1", false, true),
            ("cli-v0.13.0", "0.13.0-dev", false, true),
            ("cli-v1.0.0-rc.1", "1.0.0-beta.2", false, true),
            ("cli-v0.13.0-rc.1", "0.13.0", false, false),
            ("cli-v0.12.0", "0.0.0-dev", false, true),
            ("cli-v0.12.0", "0.13.0", true, true),
            ("cli-v0.13.0", "0.13.0", true, true),
        ];
        for (tag, installed, forced, want) in cases {
            assert_eq!(
                should_install(tag, installed, forced),
                want,
                "{tag} over {installed}, forced {forced}"
            );
        }
    }

    #[test]
    fn up_to_date_names_both_versions() {
        assert_eq!(
            up_to_date_line("0.13.1", "cli-v0.13.0", false),
            "peektrace is up to date: installed cli-v0.13.1, latest release cli-v0.13.0."
        );
        assert_eq!(
            up_to_date_line("cli-v0.13.0", "cli-v0.13.0", true),
            "peektrace is up to date: installed cli-v0.13.0, requested release cli-v0.13.0."
        );
    }
}
