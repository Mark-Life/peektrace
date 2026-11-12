//! The SQLite file both CLIs share: `<dir>/telemetry.db`, schema text as TS
//! writes it, a 30-day and 50,000-row retention applied at every open.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, params};

use crate::event::WideEvent;

pub const RETENTION_DAYS: i64 = 30;
pub const MAX_ROWS: i64 = 50_000;
const MS_PER_DAY: i64 = 86_400_000;
/// How long a write waits for another process's lock.
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(100);
pub const FILE_NAME: &str = "telemetry.db";

/// The TS DDL, byte for byte: both CLIs create the same table.
pub const DDL: [&str; 4] = [
    "CREATE TABLE IF NOT EXISTS events (\n    id TEXT PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL,\n    outcome TEXT NOT NULL, error_tag TEXT, duration_ms INTEGER NOT NULL,\n    interesting INTEGER NOT NULL, data TEXT NOT NULL)",
    "CREATE INDEX IF NOT EXISTS idx_events_ts          ON events(ts)",
    "CREATE INDEX IF NOT EXISTS idx_events_interesting ON events(interesting, ts)",
    "CREATE INDEX IF NOT EXISTS idx_events_outcome     ON events(outcome, ts)",
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("PEEKTRACE_DIR is empty")]
    EmptyDir,
}

/// The telemetry dir: `PEEKTRACE_DIR` when set, else `<home>/.peektrace`.
/// An empty `PEEKTRACE_DIR` names no dir.
pub fn dir_of(peektrace_dir: Option<&str>, home: &str) -> Result<PathBuf, StoreError> {
    match peektrace_dir {
        Some("") => Err(StoreError::EmptyDir),
        Some(d) => Ok(PathBuf::from(d)),
        None => Ok(Path::new(home).join(".peektrace")),
    }
}

/// One insert, column by column.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub ts: i64,
    pub kind: &'static str,
    pub name: String,
    pub outcome: &'static str,
    pub error_tag: Option<String>,
    pub duration_ms: f64,
    pub interesting: bool,
    pub data: String,
}

impl Row {
    pub fn of(e: &WideEvent) -> Self {
        Self {
            id: e.id.clone(),
            ts: e.ts,
            kind: e.kind.as_str(),
            name: e.name.clone(),
            outcome: e.outcome.as_str(),
            error_tag: e.outcome.error_tag().map(str::to_owned),
            duration_ms: e.duration_ms,
            interesting: e.interesting(),
            data: e.to_json(),
        }
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Create `dir` and open its `telemetry.db`, then apply the schema and
    /// the retention as of `now_ms`.
    pub fn open(dir: &Path, now_ms: i64) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(FILE_NAME))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn, now_ms)
    }

    pub fn open_in_memory(now_ms: i64) -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?, now_ms)
    }

    fn init(conn: Connection, now_ms: i64) -> Result<Self, StoreError> {
        for ddl in DDL {
            conn.execute(ddl, [])?;
        }
        let cutoff = now_ms - RETENTION_DAYS * MS_PER_DAY;
        conn.execute("DELETE FROM events WHERE ts < ?1", [cutoff])?;
        conn.execute(
            "DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY ts DESC LIMIT ?1)",
            [MAX_ROWS],
        )?;
        Ok(Self { conn })
    }

    /// Insert `rows` in one transaction.
    pub fn record(&mut self, rows: &[Row]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT OR REPLACE INTO events (id,ts,kind,name,outcome,error_tag,duration_ms,interesting,data) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )?;
            for r in rows {
                insert.execute(params![
                    r.id,
                    r.ts,
                    r.kind,
                    r.name,
                    r.outcome,
                    r.error_tag,
                    r.duration_ms,
                    i64::from(r.interesting),
                    r.data
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The stored JSON of the newest events, newest first. A negative
    /// `limit` means no limit, as SQLite reads it.
    pub fn recent(&self, limit: i64, interesting_only: bool) -> Result<Vec<String>, StoreError> {
        let sql = if interesting_only {
            "SELECT data FROM events WHERE interesting = 1 ORDER BY ts DESC LIMIT ?1"
        } else {
            "SELECT data FROM events WHERE 1 = 1 ORDER BY ts DESC LIMIT ?1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([limit], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Kind, Outcome, SpanRecord};

    const NOW: i64 = 1_800_000_000_000;

    fn success() -> WideEvent {
        WideEvent {
            id: "evt-1".to_owned(),
            trace_id: "trace-1".to_owned(),
            ts: NOW,
            kind: Kind::Cli,
            name: "sessions ls".to_owned(),
            app_version: "0.0.1".to_owned(),
            platform: "test".to_owned(),
            duration_ms: 12.0,
            attributes: r#"{"argv":"sessions ls"}"#.to_owned(),
            spans: vec![SpanRecord {
                name: "fs.read".to_owned(),
                duration_ms: 3.0,
                attributes: r#"{"path":"/x"}"#.to_owned(),
            }],
            outcome: Outcome::Success,
        }
    }

    fn failure() -> WideEvent {
        WideEvent {
            id: "evt-2".to_owned(),
            trace_id: "trace-2".to_owned(),
            ts: NOW + 1,
            name: "boom".to_owned(),
            duration_ms: 5.0,
            attributes: "{}".to_owned(),
            spans: Vec::new(),
            outcome: Outcome::Error {
                tag: "BoomError".to_owned(),
                message: "kaboom".to_owned(),
            },
            ..success()
        }
    }

    fn store() -> Store {
        Store::open_in_memory(NOW).unwrap()
    }

    fn insert(s: &Store, id: &str, ts: i64) {
        s.connection()
            .execute(
                "INSERT INTO events VALUES (?1, ?2, 'cli', 'n', 'success', NULL, 1, 0, '{}')",
                params![id, ts],
            )
            .unwrap();
    }

    fn ids(s: &Store) -> Vec<String> {
        let mut stmt = s
            .connection()
            .prepare("SELECT id FROM events ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn empty_recent() {
        assert_eq!(store().recent(10, false).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn purges_old_rows_on_open() {
        let s = store();
        insert(&s, "stale", NOW - (RETENTION_DAYS + 1) * MS_PER_DAY);
        insert(&s, "fresh", NOW);
        let s = Store::init(s.conn, NOW).unwrap();
        assert_eq!(ids(&s), ["fresh"]);
    }

    #[test]
    fn keeps_the_newest_rows() {
        let s = store();
        s.connection()
            .execute(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50005)
                 INSERT INTO events SELECT 'e' || i, ?1 + i, 'cli', 'n', 'success', NULL, 1, 0, '{}' FROM n",
                [NOW - 100_000],
            )
            .unwrap();
        let s = Store::init(s.conn, NOW).unwrap();
        let count: i64 = s
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, MAX_ROWS);
        let oldest: String = s
            .connection()
            .query_row("SELECT id FROM events ORDER BY ts LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(oldest, "e6");
    }

    #[test]
    fn round_trip_and_interesting_only() {
        let mut s = store();
        s.record(&[Row::of(&success()), Row::of(&failure())])
            .unwrap();
        assert_eq!(
            s.recent(10, false).unwrap(),
            [failure().to_json(), success().to_json()]
        );
        assert_eq!(s.recent(10, true).unwrap(), [failure().to_json()]);
        assert_eq!(s.recent(1, false).unwrap().len(), 1);
        assert_eq!(s.recent(-1, false).unwrap().len(), 2);
    }

    #[test]
    fn columns() {
        let mut s = store();
        let mut e = failure();
        e.duration_ms = 19.15175;
        s.record(&[Row::of(&e)]).unwrap();
        let (tag, interesting, kind): (String, i64, String) = s
            .connection()
            .query_row(
                "SELECT error_tag, interesting, typeof(duration_ms) FROM events WHERE id = 'evt-2'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (tag.as_str(), interesting, kind.as_str()),
            ("BoomError", 1, "real")
        );
    }

    #[test]
    fn dirs() {
        assert_eq!(
            dir_of(None, "/h").unwrap(),
            Path::new("/h").join(".peektrace")
        );
        assert_eq!(dir_of(Some("/x"), "/h").unwrap(), PathBuf::from("/x"));
        assert!(dir_of(Some(""), "/h").is_err());
    }
}
