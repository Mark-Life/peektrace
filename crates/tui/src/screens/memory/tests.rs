use std::fs;
use std::path::PathBuf;

use peektrace_core::memory::read::{AllVaults, all_vaults};
use peektrace_core::roots::Source;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::State;
use super::rows::Row;
use crate::backend::{Reply, Response, Slot};

struct Vault(PathBuf);

impl Vault {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("peektrace-tui-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self(dir)
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn load(&self) -> AllVaults {
        let root = self.0.to_string_lossy().into_owned();
        all_vaults(&[Source {
            id: "claude".to_owned(),
            label: "claude".to_owned(),
            home: root.clone(),
            projects_root: root,
        }])
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn note(name: &str) -> String {
    format!("---\nname: {name}\ndescription: d\ntype: user\n---\nbody\n")
}

fn press(s: &mut State, keys: &[KeyCode]) {
    for k in keys {
        s.key(KeyEvent::from(*k));
    }
}

fn selected(s: &State) -> Option<Row> {
    let rows = s.rows();
    rows.get(s.sel.at(rows.len())).copied()
}

#[test]
fn type_cycle_onto_a_header_hands_keys_back_to_the_list() {
    let v = Vault::new("type-cycle");
    v.write("-tmp-a/memory/a.md", &note("a"));
    let mut s = State::default();
    s.vaults.data = Some(Box::new(v.load()));
    press(&mut s, &[KeyCode::Char('j'), KeyCode::Tab]);
    assert!(s.body_focused);
    press(&mut s, &[KeyCode::Char('t')]);
    assert!(matches!(selected(&s), Some(Row::Header { .. })));
    assert!(!s.body_focused);
    press(&mut s, &[KeyCode::Char('j')]);
    assert!(matches!(selected(&s), Some(Row::Entry { .. })));
}

#[test]
fn refetch_landing_on_a_header_drops_body_focus() {
    let v = Vault::new("refetch");
    v.write("-tmp-a/memory/a1.md", &note("a1"));
    v.write("-tmp-a/memory/a2.md", &note("a2"));
    v.write("-tmp-b/memory/b1.md", &note("b1"));
    v.write("-tmp-b/memory/b2.md", &note("b2"));
    let mut s = State::default();
    s.vaults.data = Some(Box::new(v.load()));
    press(
        &mut s,
        &[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Tab],
    );
    assert!(s.body_focused);
    let Some(Row::Entry { vault, entry }) = selected(&s) else {
        panic!("an entry is selected");
    };
    let mut next = s.vaults.data.clone().unwrap();
    next.vaults[vault].entries.remove(entry);
    s.vaults.start(1);
    s.reply(Reply {
        id: 1,
        slot: Slot::Vaults,
        resp: Response::Vaults(Ok(next)),
    });
    assert!(matches!(selected(&s), Some(Row::Header { .. })));
    assert!(!s.body_focused);
}
