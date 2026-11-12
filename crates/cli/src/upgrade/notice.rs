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

/// The running binary's path as the hint reads it.
pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
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
mod tests {
    use std::cell::Cell;

    use super::*;

    /// TS `update-hint.test.ts`.
    #[test]
    fn dev_versions() {
        assert!(is_dev_version("0.0.0-dev"));
        assert!(is_dev_version("1.2.0-dev.3"));
        assert!(!is_dev_version("1.0.0-devious"));
        assert!(!is_dev_version("1.0.0"));
    }

    /// TS `update-hint.test.ts`.
    #[test]
    fn install_kinds() {
        assert!(is_npm_install(
            "/usr/lib/node_modules/peektrace/bin/peektrace"
        ));
        assert!(is_npm_install(
            r"C:\Users\x\node_modules\peektrace\peektrace.exe"
        ));
        assert!(!is_npm_install("/usr/local/bin/peektrace"));
        assert!(!is_npm_install("/x/my_node_modules/peektrace"));
        assert!(is_bun_global_install(
            "/home/u/.bun/install/global/node_modules/peektrace/bin/peektrace"
        ));
        assert!(is_bun_global_install(
            r"C:\Users\u\.bun\install\global\node_modules\peektrace\p.exe"
        ));
    }

    /// TS `update-hint.test.ts`.
    #[test]
    fn hints() {
        assert_eq!(
            update_hint("cli-v9.9.9", "/usr/local/bin/peektrace"),
            "A newer version (cli-v9.9.9) is available — run 'peektrace upgrade'"
        );
        assert_eq!(
            update_hint("cli-v1.0.0", "/u/node_modules/peektrace/bin/peektrace"),
            "A newer version (cli-v1.0.0) is available — run 'npm i -g peektrace@latest' (or 'npx peektrace@latest')"
        );
        assert_eq!(
            update_hint(
                "cli-v1.0.0",
                "/u/.bun/install/global/node_modules/peektrace/bin/peektrace"
            ),
            "A newer version (cli-v1.0.0) is available — run 'bun add -g peektrace@latest'"
        );
    }

    #[test]
    fn caches() {
        let c = Cache {
            checked_at: 1_700_000_000_000.0,
            latest_tag: Some("cli-v9.9.9".to_owned()),
        };
        assert_eq!(
            cache_json(&c),
            r#"{"checkedAt":1700000000000,"latestTag":"cli-v9.9.9"}"#
        );
        assert_eq!(parse_cache(&cache_json(&c)), Some(c));
        assert_eq!(parse_cache(r#"{"checkedAt":"x"}"#), None);
        assert_eq!(parse_cache("{not json"), None);
        assert_eq!(
            parse_cache(r#"{"checkedAt":5,"latestTag":7}"#),
            Some(Cache {
                checked_at: 5.0,
                latest_tag: None
            })
        );
        assert_eq!(
            cache_path(Some("/p"), "/h"),
            PathBuf::from("/p/update-check.json")
        );
        assert_eq!(
            cache_path(None, "/h"),
            PathBuf::from("/h/.peektrace/update-check.json")
        );
    }

    /// TS `upgrade.test.ts` `startupUpdateNotice` cache rules.
    #[test]
    fn cache_window() {
        let now = 10.0 * CACHE_TTL_MS;
        let fresh = |tag: &str| {
            Some(Cache {
                checked_at: now - 1000.0,
                latest_tag: Some(tag.to_owned()),
            })
        };
        let fetched = Cell::new(0);
        let written = Cell::new(0);
        let fetch = |tag: Option<&str>| {
            fetched.set(fetched.get() + 1);
            tag.map(str::to_owned)
        };
        let write = |_: &Cache| written.set(written.get() + 1);
        // A fresh cache skips the network.
        assert_eq!(
            available_update("0.1.0", now, fresh("cli-v9.9.9"), || fetch(None), write),
            Some("cli-v9.9.9".to_owned())
        );
        assert_eq!((fetched.get(), written.get()), (0, 0));
        // An older cached tag prints nothing.
        assert_eq!(
            available_update("0.1.0", now, fresh("cli-v0.0.1"), || fetch(None), write),
            None
        );
        // A stale cache fetches and writes back.
        let stale = Some(Cache {
            checked_at: now - CACHE_TTL_MS,
            latest_tag: Some("cli-v0.0.1".to_owned()),
        });
        assert_eq!(
            available_update("0.1.0", now, stale, || fetch(Some("cli-v1.0.0")), write),
            Some("cli-v1.0.0".to_owned())
        );
        assert_eq!((fetched.get(), written.get()), (1, 1));
        // A failure is not cached.
        assert_eq!(
            available_update("0.1.0", now, None, || fetch(None), write),
            None
        );
        assert_eq!((fetched.get(), written.get()), (2, 1));
        // Up to date.
        assert_eq!(
            available_update("1.0.0", now, None, || fetch(Some("cli-v1.0.0")), write),
            None
        );
        // A future `checkedAt` counts as fresh.
        let future = Some(Cache {
            checked_at: now + 5.0,
            latest_tag: Some("cli-v2.0.0".to_owned()),
        });
        assert_eq!(
            available_update("1.0.0", now, future, || fetch(None), write),
            Some("cli-v2.0.0".to_owned())
        );
    }

    #[test]
    fn dev_and_disabled_make_no_request() {
        let env = |version: &str, disabled| NoticeEnv {
            version: version.to_owned(),
            exe: String::new(),
            cache_path: PathBuf::from("/nonexistent-dir-xyz/update-check.json"),
            config: ReleaseConfig {
                base_url: "http://127.0.0.1:1".to_owned(),
                github_api: "http://127.0.0.1:1".to_owned(),
                pinned_version: None,
            },
            now_ms: 0.0,
            disabled,
        };
        assert_eq!(notice_line(&env("0.0.0-dev", false)), None);
        assert_eq!(notice_line(&env("1.0.0", true)), None);
    }

    /// Answer `count` requests with a releases list; returns the origin.
    fn releases_server(count: usize) -> String {
        use std::io::{BufRead as _, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            for conn in listener.incoming().take(count) {
                let Ok(mut conn) = conn else { continue };
                let _: Vec<String> = BufReader::new(conn.try_clone().expect("clone"))
                    .lines()
                    .map_while(Result::ok)
                    .take_while(|l| !l.is_empty())
                    .collect();
                let body = r#"[{"tag_name":"cli-v9.9.9"}]"#;
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        origin
    }

    /// TS `upgrade.test.ts` `startupUpdateNotice`: hint plus cache write,
    /// npm hint, unreachable stays silent.
    #[test]
    fn notice_prints_the_hint_and_writes_the_cache() {
        let dir = std::env::temp_dir().join(format!("peektrace-notice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let origin = releases_server(2);
        let env = |api: &str, exe: &str| NoticeEnv {
            version: "0.1.0".to_owned(),
            exe: exe.to_owned(),
            cache_path: dir.join("nested/update-check.json"),
            config: ReleaseConfig {
                base_url: api.to_owned(),
                github_api: api.to_owned(),
                pinned_version: None,
            },
            now_ms: 1_000.0,
            disabled: false,
        };
        assert_eq!(
            notice_line(&env(&origin, "/usr/local/bin/peektrace")),
            Some("A newer version (cli-v9.9.9) is available — run 'peektrace upgrade'".to_owned())
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("nested/update-check.json")).ok(),
            Some(r#"{"checkedAt":1000,"latestTag":"cli-v9.9.9"}"#.to_owned())
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(notice_line(&env("http://127.0.0.1:1", "/x")), None);
        assert!(!dir.exists());
        assert_eq!(
            notice_line(&env(&origin, "/u/node_modules/peektrace/bin/peektrace")),
            Some("A newer version (cli-v9.9.9) is available — run 'npm i -g peektrace@latest' (or 'npx peektrace@latest')".to_owned())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
