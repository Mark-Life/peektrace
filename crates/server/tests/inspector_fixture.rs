//! The analyses `apps/inspector/test/tool-event.test.ts` shapes, as the
//! server answers them: the shared Claude fixture redacted and raw, and the
//! tool outcomes transcript. Run with `UPDATE_TS_TYPES=1` to rewrite the
//! checked-in file after an analysis change.

mod common;

use std::path::{Path, PathBuf};

use common::{Options, SESSION, fixtures, start};
use peektrace_core::AgentRoots;
use peektrace_core::roots::Source;
use serde_json::{Value, json};

const OUT: &str = "../../apps/inspector/test/fixtures/tool-event";
const SLUG: &str = "-tmp-tool-event";
/// Stands in for the temp projects root, so the file does not change per run.
const ROOT: &str = "/tmp/projects";

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(OUT)
}

fn projects_root() -> PathBuf {
    let root = common::temp_dir("inspector-fixture");
    let dir = root.join(SLUG);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let golden = format!("sessions/projects/-Users-demo-proj/{SESSION}.jsonl");
    std::fs::copy(fixtures(&golden), dir.join(format!("{SESSION}.jsonl"))).expect("golden");
    std::fs::copy(out_dir().join("outcomes.jsonl"), dir.join("outcomes.jsonl")).expect("outcomes");
    root
}

fn generated() -> String {
    let root = projects_root();
    let s = start(&Options {
        name: "inspector-fixture",
        roots: Some(AgentRoots {
            claude: vec![Source {
                id: "claude".to_owned(),
                label: "default".to_owned(),
                home: "/nonexistent-home".to_owned(),
                projects_root: root.to_string_lossy().into_owned(),
            }],
            ..AgentRoots::default()
        }),
        ..Options::default()
    });
    let analysis = |path: &str| {
        let a = s.get(path);
        assert_eq!(a.status, 200, "{path}: {}", a.body);
        a.body.replace(&*root.to_string_lossy(), ROOT)
    };
    let parse = |text: String| serde_json::from_str::<Value>(&text).expect("analysis json");
    let all = json!({
        "golden": parse(analysis(&format!("/api/sessions/{SESSION}/analysis"))),
        "goldenRaw": parse(analysis(&format!("/api/sessions/{SESSION}/analysis?redact=false"))),
        "outcomes": parse(analysis("/api/sessions/outcomes/analysis")),
    });
    let mut text = serde_json::to_string_pretty(&all).expect("pretty");
    text.push('\n');
    text
}

#[test]
fn tool_event_analyses_are_current() {
    let path = out_dir().join("analyses.json");
    let want = generated();
    if std::env::var_os("UPDATE_TS_TYPES").is_some() {
        std::fs::write(&path, &want).expect("write fixture");
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "{} is stale: run UPDATE_TS_TYPES=1 cargo test -p peektrace-server --test inspector_fixture",
        path.display()
    );
}
