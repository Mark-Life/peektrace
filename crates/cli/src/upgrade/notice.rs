//! The "a newer version exists" hint, and the best-effort startup check
//! `serve` runs. The check makes one call to the releases API, so it is
//! skipped under `PEEKTRACE_NO_UPDATE_CHECK`, cached for a day, bounded by a
//! short timeout and silent on any failure.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use peektrace_core::js::json_out::push_str;
use peektrace_core::js::num;
use peektrace_core::js::value::JsValue;

use super::release::{ReleaseConfig, agent, compare_versions, fetch_latest_tag};

/// The check runs at most once per this window.
const CACHE_TTL_MS: f64 = 86_400_000.0;
const CHECK_TIMEOUT: Duration = Duration::from_millis(1500);
const CACHE_FILE: &str = "update-check.json";

/// `0.0.0-dev` and `1.2.0-dev.3` style builds: `/-dev(\.|$)/`.
pub fn is_dev_version(version: &str) -> bool {
    version.match_indices("-dev").any(|(at, m)| {
        let rest = &version[at + m.len()..];
        rest.is_empty() || rest.starts_with('.')
    })
}

/// The binary sits under a `node_modules` dir (npm, npx, pnpm, bun global).
pub fn is_npm_install(exe: &str) -> bool {
    exe.split(['/', '\\']).any(|seg| seg == "node_modules")
}

/// A bun global install (`~/.bun/install/global/node_modules/...`).
pub fn is_bun_global_install(exe: &str) -> bool {
    exe.replace('\\', "/").contains(".bun/install/global")
}

fn hint_command(exe: &str) -> &'static str {
    if is_bun_global_install(exe) {
        "'bun add -g peektrace@latest'"
    } else if is_npm_install(exe) {
        "'npm i -g peektrace@latest' (or 'npx peektrace@latest')"
    } else {
        "'peektrace upgrade'"
    }
}

/// The upgrade hint for how this binary was installed.
pub fn update_hint(latest: &str, exe: &str) -> String {
    format!(
        "A newer version ({latest}) is available — run {}",
        hint_command(exe)
    )
}

/// `path` with symlinks resolved, like Bun's `process.execPath`; the raw
/// path when that fails.
pub fn real_path(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// The running binary's real path as the hint reads it, so the hint and the
/// file `upgrade` replaces agree.
pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| real_path(p).to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The last check: when it ran and the tag it saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Cache {
    pub checked_at: f64,
    pub latest_tag: Option<String>,
}

/// The cache file under `PEEKTRACE_DIR`, else `~/.peektrace`.
pub fn cache_path(peektrace_dir: Option<&str>, home: &str) -> PathBuf {
    match peektrace_dir {
        Some(dir) => Path::new(dir).join(CACHE_FILE),
        None => Path::new(home).join(".peektrace").join(CACHE_FILE),
    }
}

/// A cache read from its text; `None` when it is corrupt or `checkedAt` is
/// not a number.
pub fn parse_cache(text: &str) -> Option<Cache> {
    let v = JsValue::parse(text)?;
    Some(Cache {
        checked_at: v.get("checkedAt")?.as_num()?,
        latest_tag: v
            .get("latestTag")
            .and_then(JsValue::as_str)
            .map(str::to_owned),
    })
}

pub fn cache_json(c: &Cache) -> String {
    let mut out = format!(
        "{{\"checkedAt\":{},\"latestTag\":",
        num::to_json(c.checked_at)
    );
    match &c.latest_tag {
        Some(t) => push_str(&mut out, t),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

/// The newest tag when it is newer than `current`. A cache younger than a
/// day is used as is; otherwise `fetch` runs and only a found tag is
/// written back, so a failure never opens the window.
pub fn available_update(
    current: &str,
    now_ms: f64,
    cache: Option<Cache>,
    fetch: impl FnOnce() -> Option<String>,
    write: impl FnOnce(&Cache),
) -> Option<String> {
    let latest = match cache {
        Some(c) if now_ms - c.checked_at < CACHE_TTL_MS => c.latest_tag,
        _ => {
            let found = fetch();
            if let Some(tag) = &found {
                write(&Cache {
                    checked_at: now_ms,
                    latest_tag: Some(tag.clone()),
                });
            }
            found
        }
    }?;
    (compare_versions(&latest, current) == Ordering::Greater).then_some(latest)
}

/// What the startup check reads from the process.
pub struct NoticeEnv {
    pub version: String,
    pub exe: String,
    pub cache_path: PathBuf,
    pub config: ReleaseConfig,
    pub now_ms: f64,
    /// `PEEKTRACE_NO_UPDATE_CHECK` is set and not empty.
    pub disabled: bool,
}

/// The hint line to print, if a newer release exists. Makes no request
/// and reads no cache for dev builds or when disabled.
pub fn notice_line(env: &NoticeEnv) -> Option<String> {
    if env.disabled || is_dev_version(&env.version) {
        return None;
    }
    let cache = std::fs::read(&env.cache_path)
        .ok()
        .and_then(|b| parse_cache(&String::from_utf8_lossy(&b)));
    let client = agent(Some(CHECK_TIMEOUT));
    let latest = available_update(
        &env.version,
        env.now_ms,
        cache,
        || fetch_latest_tag(&client, &env.config).ok().flatten(),
        |c| {
            if let Some(dir) = env.cache_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&env.cache_path, cache_json(c));
        },
    )?;
    Some(update_hint(&latest, &env.exe))
}

/// Run the check on its own thread and hand the line to `print`, unless it
/// misses the deadline. Never blocks the caller.
pub fn spawn_notice(env: NoticeEnv, print: impl FnOnce(&str) + Send + 'static) {
    if env.disabled || is_dev_version(&env.version) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("update-check".to_owned())
        .spawn(move || {
            let (tx, rx) = mpsc::channel();
            let worker = std::thread::Builder::new().spawn(move || {
                let _ = tx.send(notice_line(&env));
            });
            if worker.is_err() {
                return;
            }
            if let Ok(Some(line)) = rx.recv_timeout(CHECK_TIMEOUT) {
                print(&line);
            }
        });
}

#[cfg(test)]
#[path = "notice_tests.rs"]
mod tests;
