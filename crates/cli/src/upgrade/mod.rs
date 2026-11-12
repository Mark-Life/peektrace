//! `peektrace upgrade`: resolve the target release tag, download the new
//! binary, verify its sha256, then atomically replace the installed one.
//! `--check` only reports. Windows cannot replace a running `.exe`, so it
//! points at the installer.

pub mod install;
pub mod notice;
pub mod release;

use std::cmp::Ordering;
use std::path::PathBuf;

use self::release::{Detection, Os, ReleaseConfig, SUPPORTED_PLATFORMS};
use crate::args::UpgradeArgs;
use crate::ls::UserError;
use crate::render::bytes;

/// Hidden test hook: the file `upgrade` replaces instead of the running
/// binary.
pub const TARGET_ENV: &str = "PEEKTRACE_UPGRADE_TARGET";

const WINDOWS_INSTALL_HINT: &str = "Automatic in-place upgrade is not supported on Windows (a running .exe can't replace itself).\n\
Re-run the installer to update:\n  \
irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/scripts/install.ps1 | iex";

/// What `upgrade` reads from the process.
pub struct UpgradeEnv {
    pub version: &'static str,
    pub config: ReleaseConfig,
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
            config: ReleaseConfig::from_env(var),
            host: release::host_asset(),
            exe: notice::exe_path(),
            target,
        }
    }
}

/// Run `upgrade`; each stdout line goes to `print` as it is decided, so
/// `Fetching ...` shows before the download.
pub fn run(
    args: &UpgradeArgs,
    env: &UpgradeEnv,
    print: &mut dyn FnMut(&str),
) -> Result<(), UserError> {
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
    let tag = release::resolve_target_tag(&agent, &env.config, pinned)?;
    let comparison = release::compare_versions(&tag, env.version);
    if args.check {
        if comparison == Ordering::Greater {
            print(&notice::update_hint(&tag, &env.exe));
        } else {
            print(&format!(
                "peektrace is up to date ({}).",
                release::normalize_tag(env.version)
            ));
        }
        return Ok(());
    }
    if os == Os::Windows {
        print(WINDOWS_INSTALL_HINT);
        return Ok(());
    }
    let forced = pinned.is_some() || env.config.pinned_version.is_some();
    if comparison == Ordering::Equal && !forced {
        print(&format!("Already on the latest version ({tag})."));
        return Ok(());
    }
    let Some(target) = &env.target else {
        return Err(UserError::Invalid(
            "Cannot find the running peektrace binary to replace.".to_owned(),
        ));
    };
    print(&format!("Fetching {asset} ({tag})..."));
    let size = install::perform_upgrade(&agent, &env.config, &tag, asset, target)?;
    print(&format!(
        "Upgraded to {tag} (downloaded {}). It takes effect on the next launch.",
        bytes(size as u64)
    ));
    Ok(())
}
