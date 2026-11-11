//! `OpenCode`'s SQLite stores: every `opencode*.db` in the data dir, opened
//! read-only. The SQL matches the TS reader word for word, so SQLite orders
//! the rows the same way.

use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};
use tracing::debug;

use super::session::{
    Cell, Message, Session, SessionInfo, hydrate_message, hydrate_part, parse_object, text_cell,
};
use crate::js::value::{JsObject, JsValue};
use crate::paths::PathStyle;

/// How long a read waits on a writer's lock before the DB is skipped.
const BUSY_TIMEOUT: Duration = Duration::from_secs(1);

const SESSION_BY_ID: &str = "SELECT * FROM session WHERE id = ?";
const MESSAGES: &str = "SELECT id, session_id, time_created, data FROM message WHERE session_id = ? ORDER BY COALESCE(json_extract(data, '$.time.created'), time_created), id";
const PARTS: &str = "SELECT id, message_id, session_id, data FROM part WHERE session_id = ? ORDER BY message_id, id";
const SESSION_REFS: &str = "SELECT id, directory, time_updated FROM session";

/// Names in `dir`, in directory order, UTF-8 names only.
pub(super) fn entries(dir: &str) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// One `opencode*.db` file, open read-only.
pub(super) struct Db {
    path: String,
    conn: Connection,
}

/// Every `opencode*.db` in `data_dir`, in directory order (not sorted: the
/// first DB that holds an id wins). A file that fails to open is skipped.
pub(super) fn open_dbs(data_dir: &str) -> Vec<Db> {
    entries(data_dir)
        .into_iter()
        .filter(|name| {
            #[expect(
                clippy::case_sensitive_file_extension_comparisons,
                reason = "TS matches `.db` exactly"
            )]
            let db = name.ends_with(".db");
            name.starts_with("opencode") && db
        })
        .filter_map(|name| {
            let path = PathStyle::NATIVE.join(&[data_dir, &name]);
            let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
            let opened = Connection::open_with_flags(&path, flags).and_then(|conn| {
                conn.busy_timeout(BUSY_TIMEOUT)?;
                Ok(conn)
            });
            match opened {
                Ok(conn) => Some(Db { path, conn }),
                Err(e) => {
                    debug!(db = %path, reason = %e, "opencode db skipped");
                    None
                }
            }
        })
        .collect()
}

/// A column value: integers as `f64`, text decoded lossily.
fn cell(v: ValueRef<'_>) -> Cell {
    match v {
        ValueRef::Null => Cell::Null,
        #[expect(
            clippy::cast_precision_loss,
            reason = "bun:sqlite reads integers as f64"
        )]
        ValueRef::Integer(n) => Cell::Num(n as f64),
        ValueRef::Real(x) => Cell::Num(x),
        ValueRef::Text(b) => text_cell(&String::from_utf8_lossy(b)),
        ValueRef::Blob(_) => Cell::Other,
    }
}

fn cell_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Cell> {
    row.get_ref(i).map(cell)
}

/// A session id and the directory listing shows for it.
pub(super) struct DbRef {
    pub id: String,
    pub directory: Option<String>,
}

fn list_refs(db: &Db) -> rusqlite::Result<Vec<DbRef>> {
    let mut stmt = db.conn.prepare(SESSION_REFS)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        if let Cell::Text(id) = cell_at(row, 0)? {
            out.push(DbRef {
                id,
                directory: cell_at(row, 1)?.opt_str(),
            });
        }
    }
    Ok(out)
}

/// Every session row across `dbs`, the first DB that lists an id winning.
/// A DB whose query fails is skipped.
pub(super) fn list_db_refs(dbs: &[Db]) -> Vec<DbRef> {
    let mut seen = std::collections::HashSet::new();
    dbs.iter()
        .flat_map(|db| {
            list_refs(db).unwrap_or_else(|e| {
                debug!(db = %db.path, reason = %e, "opencode db not listed");
                Vec::new()
            })
        })
        .filter(|r| seen.insert(r.id.clone()))
        .collect()
}

fn info_from_row(id: &str, row: &HashMap<String, Cell>) -> SessionInfo {
    let col = |k: &str| row.get(k).unwrap_or(&Cell::Null);
    let model = col("model");
    SessionInfo {
        id: id.to_owned(),
        time_created: col("time_created").num(),
        time_updated: col("time_updated").num(),
        cost: col("cost").num(),
        tokens_input: col("tokens_input").num(),
        tokens_output: col("tokens_output").num(),
        tokens_reasoning: col("tokens_reasoning").num(),
        tokens_cache_read: col("tokens_cache_read").num(),
        tokens_cache_write: col("tokens_cache_write").num(),
        project_id: col("project_id").opt_str(),
        directory: col("directory").opt_str(),
        title: col("title").opt_str(),
        version: col("version").opt_str(),
        model: model
            .as_text()
            .and_then(parse_object)
            .map(JsValue::Obj)
            .or_else(|| model.opt_str().map(JsValue::Str)),
        agent: col("agent").opt_str(),
    }
}

fn data_object(c: &Cell) -> Option<JsObject> {
    c.as_text().and_then(parse_object)
}

fn load_messages(db: &Db, id: &str) -> rusqlite::Result<Vec<Message>> {
    let mut stmt = db.conn.prepare(MESSAGES)?;
    let mut rows = stmt.query([id])?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next()? {
        if let Cell::Text(message_id) = cell_at(row, 0)? {
            let data = hydrate_message(
                data_object(&cell_at(row, 3)?),
                &message_id,
                id,
                cell_at(row, 2)?.num(),
            );
            messages.push((message_id, data));
        }
    }
    let mut by_message: HashMap<String, Vec<JsObject>> = HashMap::new();
    let mut stmt = db.conn.prepare(PARTS)?;
    let mut rows = stmt.query([id])?;
    while let Some(row) = rows.next()? {
        if let Cell::Text(message_id) = cell_at(row, 1)? {
            let part = hydrate_part(
                data_object(&cell_at(row, 3)?),
                cell_at(row, 0)?.to_js(),
                id,
                &message_id,
            );
            by_message.entry(message_id).or_default().push(part);
        }
    }
    Ok(messages
        .into_iter()
        .map(|(message_id, data)| Message {
            data,
            parts: by_message.remove(&message_id).unwrap_or_default(),
        })
        .collect())
}

fn load_one(db: &Db, id: &str) -> rusqlite::Result<Option<Session>> {
    let mut stmt = db.conn.prepare(SESSION_BY_ID)?;
    let names: Vec<String> = stmt.column_names().map(str::to_owned).collect();
    let mut rows = stmt.query([id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let cells = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| Ok((name, cell_at(row, i)?)))
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(Some(Session {
        info: info_from_row(id, &cells),
        messages: load_messages(db, id)?,
    }))
}

/// The session from the first DB that holds it and loads whole. A DB that
/// fails (bad file, missing table, malformed message JSON) is skipped.
pub(super) fn load_from_dbs(dbs: &[Db], id: &str) -> Option<Session> {
    dbs.iter().find_map(|db| match load_one(db, id) {
        Ok(found) => found,
        Err(e) => {
            debug!(db = %db.path, reason = %e, "opencode db skipped");
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn sqlite_built_without_memstatus() -> rusqlite::Result<()> {
        // Memory accounting serialises parallel readers on one global mutex.
        let conn = Connection::open_in_memory()?;
        let off: bool = conn.query_row(
            "SELECT sqlite_compileoption_used('DEFAULT_MEMSTATUS=0')",
            [],
            |r| r.get(0),
        )?;
        assert!(off, "build SQLite with -DSQLITE_DEFAULT_MEMSTATUS=0");
        Ok(())
    }
}
