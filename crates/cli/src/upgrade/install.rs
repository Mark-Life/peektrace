//! Download, verify, then replace. The bytes that land on disk are checked
//! against the `SHA256SUMS` entry for the uncompressed asset before they
//! replace anything; a mismatch or a missing entry leaves the target as it
//! was.

use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::MultiGzDecoder;
use sha2::{Digest as _, Sha256};

use super::release::{FetchError, ReleaseConfig, get_bytes, parse_checksum};
use crate::ls::UserError;

#[cfg(unix)]
const EXEC_MODE: u32 = 0o755;

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The inflated bytes, or `None` when they are not gzip.
fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    MultiGzDecoder::new(bytes).read_to_end(&mut out).ok()?;
    Some(out)
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, UserError> {
    get_bytes(agent, url, &[]).map_err(|e| {
        let cause = match e {
            FetchError::Status(n) => format!("Error: download responded {n}"),
            FetchError::Other(m) => m,
        };
        UserError::Invalid(format!("Download failed ({url}): {cause}"))
    })
}

fn fetch_text(agent: &ureq::Agent, url: &str) -> Result<String, UserError> {
    let bytes = get_bytes(agent, url, &[]).map_err(|e| {
        let cause = match e {
            FetchError::Status(n) => format!("Error: fetch responded {n}"),
            FetchError::Other(m) => m,
        };
        UserError::Invalid(format!("Fetch failed ({url}): {cause}"))
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The binary for `tag`: the `.gz` asset when it downloads and inflates,
/// else the raw one.
fn download_full(
    agent: &ureq::Agent,
    config: &ReleaseConfig,
    tag: &str,
    asset: &str,
) -> Result<Vec<u8>, UserError> {
    let url = config.asset_url(tag, asset);
    let inflated = get_bytes(agent, &format!("{url}.gz"), &[])
        .ok()
        .and_then(|gz| gunzip(&gz));
    match inflated {
        Some(bytes) => Ok(bytes),
        None => download(agent, &url),
    }
}

/// A scratch name beside the target, so the rename stays on one file
/// system.
fn temp_path(target: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seed = format!("{nanos}:{}:{}", std::process::id(), target.display());
    let hex = sha256_hex(seed.as_bytes());
    let name = format!(
        ".peektrace-upgrade-{}-{}-{}-{}-{}.tmp",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    target.parent().unwrap_or(Path::new(".")).join(name)
}

#[cfg(unix)]
fn create_exec(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(EXEC_MODE)
        .open(path)
}

#[cfg(not(unix))]
fn create_exec(path: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn set_exec(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(EXEC_MODE))
}

#[cfg(not(unix))]
fn set_exec(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Write `bytes` to a temp file beside `target`, fsync it, rename it over
/// the target, then set the exec bits. The temp file is removed on any
/// failure; the old binary stays whole until the rename.
pub fn replace_binary(bytes: &[u8], target: &Path) -> Result<(), UserError> {
    let tmp = temp_path(target);
    let step = |op: &str, path: &Path, e: std::io::Error| format!("{e}, {op} '{}'", path.display());
    let result = (|| {
        let mut file = create_exec(&tmp).map_err(|e| step("open", &tmp, e))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| step("write", &tmp, e))?;
        drop(file);
        fs::rename(&tmp, target).map_err(|e| step("rename", &tmp, e))?;
        set_exec(target).map_err(|e| step("chmod", target, e))
    })();
    result.map_err(|cause| {
        let _ = fs::remove_file(&tmp);
        UserError::Invalid(format!("Failed to replace {}: {cause}", target.display()))
    })
}

/// Download, verify and install `tag`'s `asset` over `target`. Returns the
/// size of the installed binary.
pub fn perform_upgrade(
    agent: &ureq::Agent,
    config: &ReleaseConfig,
    tag: &str,
    asset: &str,
    target: &Path,
) -> Result<usize, UserError> {
    let bytes = download_full(agent, config, tag, asset)?;
    let sums = fetch_text(agent, &config.checksums_url(tag))?;
    let expected = parse_checksum(&sums, asset).ok_or_else(|| {
        UserError::Invalid(format!(
            "No SHA256SUMS entry for {asset}; refusing to install."
        ))
    })?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(UserError::Invalid(format!(
            "Checksum mismatch for {asset} (expected {expected}, got {actual}). Aborting without installing."
        )));
    }
    replace_binary(&bytes, target)?;
    Ok(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests() {
        assert_eq!(
            sha256_hex(&[0x70, 0x65, 0x65, 0x6b, 9, 9, 9]),
            "811099c456c4276ef6135cb3ff7ab57cbe3b8d9a4916676621da4664ee78bcb2"
        );
    }

    #[test]
    fn gzip_or_not() {
        use flate2::write::GzEncoder;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"peek").ok();
        let gz = enc.finish().unwrap_or_default();
        assert_eq!(gunzip(&gz), Some(b"peek".to_vec()));
        assert_eq!(gunzip(b"not gzip"), None);
    }

    #[test]
    fn temp_names_sit_beside_the_target() {
        let t = temp_path(Path::new("/a/b/peektrace"));
        let name = t.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert_eq!(t.parent(), Some(Path::new("/a/b")));
        assert!(name.starts_with(".peektrace-upgrade-"));
        assert_eq!(t.extension().and_then(|e| e.to_str()), Some("tmp"));
        assert_eq!(name.len(), ".peektrace-upgrade-".len() + 36 + 4);
    }

    #[test]
    fn replace_into_a_missing_dir_fails_cleanly() {
        let Err(UserError::Invalid(m)) =
            replace_binary(b"x", Path::new("/nonexistent-dir-xyz/peektrace"))
        else {
            panic!("expected a failure");
        };
        assert!(
            m.starts_with("Failed to replace /nonexistent-dir-xyz/peektrace: "),
            "{m}"
        );
    }
}
