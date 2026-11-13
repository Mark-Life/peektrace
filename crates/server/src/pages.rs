//! The last few session analyses, cut into pages, so the page, index and
//! search requests of one open session do not each re-read the transcript.
//! Concurrent requests for one key wait on one build; failed builds are not
//! kept.

use std::sync::{Arc, Mutex, PoisonError};

use peektrace_core::sessions::inputs::Input;
use peektrace_core::sessions::pages::SessionPages;
use tokio::sync::OnceCell;

const SLOTS: usize = 4;

/// What one cached analysis was built from.
#[derive(Debug, Clone, PartialEq)]
pub struct PagesKey {
    pub id: String,
    pub parent: Option<String>,
    pub window: Option<f64>,
    pub dumb_zone: Option<f64>,
    pub redact: bool,
}

/// One analysis and what it was built from: every path it read
/// (transcript first), stamped before the read, and the sessions watch
/// version.
#[derive(Debug)]
pub struct Built {
    pub pages: SessionPages,
    version: u64,
    inputs: Vec<Input>,
}

impl Built {
    /// `inputs` must be stamped before the build reads them: stamping after
    /// would let a write during the build pass as already seen.
    pub const fn new(pages: SessionPages, version: u64, inputs: Vec<Input>) -> Self {
        Self {
            pages,
            version,
            inputs,
        }
    }

    /// Still valid: its inputs are unchanged, which also catches appends
    /// when the watcher is not running. Other sessions' writes bump the
    /// watch version too, so a JSONL transcript ignores it. An `OpenCode`
    /// store is shared by every session in it, so it also needs the
    /// version to hold.
    fn fresh(&self, version: u64) -> bool {
        let jsonl = self.pages.analysis().parsed.provider.as_str() != "opencode"
            && self.inputs.first().is_some_and(|i| i.stamp.is_some());
        self.inputs.iter().all(Input::unchanged) && (jsonl || self.version == version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Hit,
    Miss,
}

impl Lookup {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
        }
    }
}

type Cell = Arc<OnceCell<Arc<Built>>>;

/// Most recently used first.
#[derive(Debug, Default)]
pub struct PagesCache {
    slots: Mutex<Vec<(PagesKey, Cell)>>,
}

impl PagesCache {
    /// The cell for `key`, moved to the front, or a new empty one.
    fn cell(&self, key: &PagesKey) -> Cell {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots
            .iter()
            .position(|(k, _)| k == key)
            .map_or_else(|| (key.clone(), Cell::default()), |i| slots.remove(i));
        let cell = Arc::clone(&slot.1);
        slots.insert(0, slot);
        slots.truncate(SLOTS);
        cell
    }

    fn evict(&self, key: &PagesKey, cell: &Cell) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        slots.retain(|(k, c)| k != key || !Arc::ptr_eq(c, cell));
    }

    /// The analysis for `key` at watch `version`, built by `build` when it
    /// is missing or stale.
    pub async fn get<E, Fut>(
        &self,
        key: &PagesKey,
        version: u64,
        build: impl FnOnce() -> Fut,
    ) -> Result<(Arc<Built>, Lookup), E>
    where
        Fut: Future<Output = Result<Built, E>>,
    {
        let mut cell = self.cell(key);
        if let Some(b) = cell.get() {
            if b.fresh(version) {
                return Ok((Arc::clone(b), Lookup::Hit));
            }
            self.evict(key, &cell);
            cell = self.cell(key);
        }
        let mut ran = false;
        let entry = cell
            .get_or_try_init(|| {
                ran = true;
                async { build().await.map(Arc::new) }
            })
            .await?;
        Ok((
            Arc::clone(entry),
            if ran { Lookup::Miss } else { Lookup::Hit },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_core::sessions::inputs::stamp_all;

    fn key(id: &str) -> PagesKey {
        PagesKey {
            id: id.to_owned(),
            parent: None,
            window: None,
            dumb_zone: None,
            redact: true,
        }
    }

    fn empty_pages(path: &str) -> SessionPages {
        pages_of(peektrace_core::codex::parse_codex_session("", path, "s"))
    }

    fn pages_of(parsed: peektrace_core::sessions::model::ParsedSession) -> SessionPages {
        let a = peektrace_core::sessions::analyze::analyze(
            parsed,
            peektrace_core::sessions::analyze::AnalyzeOptions {
                dumb_zone_fraction: None,
                on_disk_context_files: Vec::new(),
                window: None,
                local: &peektrace_core::js::time::Utc,
            },
        );
        SessionPages::new(a)
    }

    async fn get(cache: &PagesCache, k: &PagesKey, version: u64, path: &str) -> Result<Lookup, ()> {
        let path = path.to_owned();
        cache
            .get(k, version, || async move {
                let pages = empty_pages(&path);
                Ok(Built::new(pages, version, stamp_all([path])))
            })
            .await
            .map(|(_, l)| l)
    }

    #[tokio::test]
    async fn hits_until_the_file_moves() {
        let dir = std::env::temp_dir().join(format!("peektrace-pages-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let file = dir.join("t.jsonl");
        std::fs::write(&file, "{}\n").expect("write");
        let path = file.to_string_lossy().into_owned();
        let cache = PagesCache::default();
        let k = key("a");
        assert_eq!(get(&cache, &k, 0, &path).await, Ok(Lookup::Miss));
        assert_eq!(get(&cache, &k, 0, &path).await, Ok(Lookup::Hit));
        // Another session's write bumps the version; this file did not move.
        assert_eq!(get(&cache, &k, 1, &path).await, Ok(Lookup::Hit));
        std::fs::write(&file, "{}\n{}\n").expect("append");
        assert_eq!(get(&cache, &k, 1, &path).await, Ok(Lookup::Miss));
        assert_eq!(get(&cache, &k, 1, &path).await, Ok(Lookup::Hit));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_append_during_the_build_misses_next_time() {
        let dir = std::env::temp_dir().join(format!("peektrace-pages-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let file = dir.join("t.jsonl");
        std::fs::write(&file, "{}\n").expect("write");
        let path = file.to_string_lossy().into_owned();
        let cache = PagesCache::default();
        let k = key("race");
        let built = cache
            .get(&k, 0, || async {
                let inputs = stamp_all([path.clone()]);
                let pages = empty_pages(&path);
                std::fs::write(&file, "{}\n{}\n").expect("append");
                Ok::<_, ()>(Built::new(pages, 0, inputs))
            })
            .await
            .map(|(_, l)| l);
        assert_eq!(built, Ok(Lookup::Miss));
        assert_eq!(get(&cache, &k, 1, &path).await, Ok(Lookup::Miss));
        assert_eq!(get(&cache, &k, 1, &path).await, Ok(Lookup::Hit));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_opencode_store_write_misses_without_a_watch_bump() {
        let dir = std::env::temp_dir().join(format!("peektrace-pages-oc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db = dir.join("opencode.db");
        std::fs::write(&db, "x").expect("write");
        let inputs = vec![
            dir.to_string_lossy().into_owned(),
            db.to_string_lossy().into_owned(),
        ];
        let path = format!("{}#ses_1", inputs[0]);
        let cache = PagesCache::default();
        let k = key("oc");
        let build = || async {
            let parsed = peektrace_core::opencode::parse_opencode_session(None, &path, "ses_1");
            Ok::<_, ()>(Built::new(pages_of(parsed), 0, stamp_all(inputs.clone())))
        };
        let lookup = |r: Result<(Arc<Built>, Lookup), ()>| r.map(|(_, l)| l);
        assert_eq!(lookup(cache.get(&k, 0, build).await), Ok(Lookup::Miss));
        assert_eq!(lookup(cache.get(&k, 0, build).await), Ok(Lookup::Hit));
        std::fs::write(&db, "xy").expect("write");
        assert_eq!(lookup(cache.get(&k, 0, build).await), Ok(Lookup::Miss));
        assert_eq!(lookup(cache.get(&k, 1, build).await), Ok(Lookup::Miss));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn keeps_four_and_drops_failures() {
        let cache = PagesCache::default();
        for id in ["a", "b", "c", "d", "e"] {
            assert_eq!(get(&cache, &key(id), 0, "/nope").await, Ok(Lookup::Miss));
        }
        assert_eq!(get(&cache, &key("e"), 0, "/nope").await, Ok(Lookup::Hit));
        // No file to stamp: a watch bump is the only sign of change.
        assert_eq!(get(&cache, &key("e"), 1, "/nope").await, Ok(Lookup::Miss));
        assert_eq!(get(&cache, &key("a"), 0, "/nope").await, Ok(Lookup::Miss));
        let failed = cache
            .get(&key("f"), 0, || async { Err::<Built, _>("boom") })
            .await;
        assert!(failed.is_err());
        assert_eq!(get(&cache, &key("f"), 0, "/nope").await, Ok(Lookup::Miss));
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_build() {
        let cache = Arc::new(PagesCache::default());
        let builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = (0..8).map(|_| {
            let cache = Arc::clone(&cache);
            let builds = Arc::clone(&builds);
            tokio::spawn(async move {
                cache
                    .get(&key("x"), 0, || async move {
                        builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        Ok::<_, ()>(Built::new(empty_pages("/nope"), 0, Vec::new()))
                    })
                    .await
                    .map(|(_, l)| l)
            })
        });
        let mut misses = 0;
        for call in calls.collect::<Vec<_>>() {
            misses += usize::from(matches!(call.await, Ok(Ok(Lookup::Miss))));
        }
        assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(misses, 1);
    }
}
