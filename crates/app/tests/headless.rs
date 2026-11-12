//! Headless renders of each scene on the synthetic HOME. The element tree,
//! normalised, is compared with `crates/ui/tests/goldens/<scene>.tree.json`
//! (`UPDATE_GOLDENS=1` rewrites them). PNGs land in `target/rune-snaps` for
//! review; they are not compared, since Metal and lavapipe raster differ.

#[path = "../../ui/tests/common/home.rs"]
mod home;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn goldens() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ui/tests/goldens")
}

fn snaps() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rune-snaps");
    std::fs::create_dir_all(&dir).expect("mkdir snaps");
    dir
}

/// Keys, roles, labels and texts in order; rects to whole points.
fn normalise(tree: &Value) -> Value {
    let elements = tree["elements"].as_array().cloned().unwrap_or_default();
    let rows: Vec<Value> = elements
        .iter()
        .map(|e| {
            let rect: Vec<i64> = e["rect"]
                .as_array()
                .map(|r| {
                    #[expect(clippy::cast_possible_truncation, reason = "points")]
                    r.iter()
                        .map(|v| v.as_f64().unwrap_or(0.0).round() as i64)
                        .collect()
                })
                .unwrap_or_default();
            let mut row = json!({ "role": e["role"], "rect": rect });
            for k in ["key", "label", "text"] {
                if !e[k].is_null() {
                    row[k] = e[k].clone();
                }
            }
            row
        })
        .collect();
    json!({ "title": tree["title"], "elements": rows })
}

fn render(home: &Path, scene: &str, theme: &str, extra: &[&str]) -> Value {
    let out = snaps();
    let tree = out.join(format!("{scene}.tree.json"));
    let png = out.join(format!("{scene}.png"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_peektrace-app"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(home::env(home))
        .args([
            "--size", "1440x900", "--theme", theme, "--scene", scene, "--tree",
        ])
        .arg(&tree)
        .arg("--snap")
        .arg(&png)
        .args(extra);
    for key in ["VK_ICD_FILENAMES", "WGPU_BACKEND", "WGPU_ADAPTER_NAME"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    let status = cmd.status().expect("run peektrace-app");
    assert!(status.success(), "{scene}: {status}");
    let text = std::fs::read_to_string(&tree).expect("read tree");
    normalise(&serde_json::from_str(&text).expect("tree json"))
}

fn check(name: &str, got: &Value) {
    let path = goldens().join(format!("{name}.tree.json"));
    let text = format!("{}\n", serde_json::to_string_pretty(got).expect("json"));
    if std::env::var_os("UPDATE_GOLDENS").is_some()
        || !path.exists() && std::env::var_os("CI").is_none()
    {
        std::fs::create_dir_all(goldens()).expect("mkdir goldens");
        std::fs::write(&path, text).expect("write golden");
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        want == text,
        "{name}: tree differs from {}; rerun with UPDATE_GOLDENS=1 and review",
        path.display()
    );
}

#[test]
fn scenes_match_goldens() {
    let full = home::build_home();
    let session = format!("session={}", home::LONG_ID);
    for (scene, theme, extra) in [
        ("list", "dark", vec![]),
        ("list-filtered", "dark", vec![]),
        ("detail-top", "dark", vec![session.as_str()]),
        ("detail-chart-hover", "dark", vec![session.as_str()]),
        ("history-open", "dark", vec![session.as_str()]),
        ("stub-stats", "dark", vec![]),
        ("light", "light", vec![session.as_str()]),
    ] {
        let got = render(&full, scene, theme, &extra);
        check(scene, &got);
    }
    let empty = home::temp_home("empty");
    check("empty-home", &render(&empty, "list", "dark", &[]));
}
