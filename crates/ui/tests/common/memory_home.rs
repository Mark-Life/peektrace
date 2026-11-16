//! Memory trees for the app's tests and goldens: one home with every vault
//! state and a small set of vaults
//! for the headless scenes. Mtimes are pinned, so dates and sort ties hold.

#![allow(dead_code, reason = "each test binary uses part of it")]

use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `2026-11-11T16:42:21.851Z`, as the harness pins it.
pub const FIXED_MS: u64 = 1_794_415_341_851;
/// The seed vault's project dir.
pub const SEED: &str = "-Users-demo-seed";

fn fixed() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(FIXED_MS)
}

pub fn pin(path: &Path, at: SystemTime) {
    let f = File::options().write(true).open(path).expect("open to pin");
    f.set_times(FileTimes::new().set_accessed(at).set_modified(at))
        .expect("pin mtime");
}

pub fn put(path: &Path, body: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
    pin(path, fixed());
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/memory")
}

/// `---\nname: \xff\xfe\n---\nbody\n`: invalid UTF-8 in a name.
const BAD_UTF8: &[u8] = b"---\nname: \xff\xfe\n---\nbody\n";

fn proj_a(a: &Path) {
    put(
        &a.join("nested.md"),
        "---\nname: nested\ndescription: A note with nested metadata\nmetadata:  \n  node_type: insight\n  type: user\n  name: inner-name\n  originSessionId: \"abc123\"\n  modified: 2026-01-01\n---\n# Nested\n\nLinks [[flat]] and [[Flat#section|alias]] and [[missing-note]].\n",
    );
    put(
        &a.join("flat.md"),
        "---\ntype: feedback\nname: flat\ndescription: \"A quoted, legacy: description\"\n1: numeric key\nzeta: z\n__proto__: proto\nalpha: 'a'\ntype: project\n---\nSee [b](sub/dir/Nested.md#x), [ext](https://x.dev/a.md), [anchor](#top), ![img](pic.png), [mail](mailto:a@b).\n",
    );
    put(
        &a.join("none.md"),
        "No frontmatter here, links to [[nested]].\r\nSecond line\r\n",
    );
    put(
        &a.join("unterminated.md"),
        "---\nname: broken\nno closing fence\n",
    );
    put(
        &a.join("crlf.md"),
        "---\r\nname: crlf\r\ndescription: Windows\r\ntype: reference\r\n---\r\nBody [[nested]]\r\n",
    );
    put(
        &a.join("bom.md"),
        "\u{feff}---\nname: bom\ntype: user\n---\nBOM body\n",
    );
    put(&a.join("badutf8.md"), BAD_UTF8);
    put(
        &a.join("weird-type.md"),
        "---\nname: weird\ntype: 42\n---\nx\n",
    );
    put(
        &a.join("self.md"),
        "---\nname: self\ntype: user\n---\n[[self]] [[nestd]] [[ne]]\n",
    );
    put(
        &a.join("Upper.md"),
        "---\ntype: user\n---\nUpper body [[upper]]\n",
    );
    put(&a.join("notes.txt"), "stray\n");
    put(&a.join("zz.json"), "{}\n");
    fs::create_dir_all(a.join("subdir.md")).expect("mkdir");
    unreadable_and_link(a);
    put(
        &a.join("MEMORY.md"),
        [
            "# Index",
            "",
            "- [Nested](nested.md) — nested one",
            "- [Flat](flat.md) – en dash hook",
            "* [None](none.md) · middle dot",
            "- [Crlf](crlf.md) -- double dash",
            "- [Bom](./bom.md#frag)",
            "- [[self]] - wiki hook",
            "- [[Upper|U]]",
            "- [Ghost](ghost.md) — missing file",
            "- [[nested-ghost]]",
            "- bare bullet with no link",
            "- [Ext](https://example.com/x.md) — external",
            "- [Empty hook](weird-type.md) —   ",
            "- [Sep](self.md) — hook more",
            "prose line",
            "",
        ]
        .join("\n"),
    );
}

/// An unreadable file and a symlink. POSIX only: Windows builds these
/// tests but never runs them.
#[cfg(unix)]
fn unreadable_and_link(a: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    put(&a.join("locked.md"), "---\nname: locked\n---\nsecret\n");
    fs::set_permissions(a.join("locked.md"), fs::Permissions::from_mode(0o000)).expect("chmod");
    std::os::unix::fs::symlink(a.join("flat.md"), a.join("link.md")).expect("symlink");
}

#[cfg(not(unix))]
const fn unreadable_and_link(_: &Path) {}

/// A small memory vault plus both `fixtures/memory` files.
fn seed_vault(dir: &Path) {
    put(
        &dir.join("a.md"),
        "---\nname: a\ndescription: First note\ntype: user\n---\nLinks to [[b]] here.\n",
    );
    put(
        &dir.join("b.md"),
        "---\nname: b\ndescription: Second note\ntype: project\n---\nNo links here.\n",
    );
    put(
        &dir.join("orphan.md"),
        "---\nname: orphan\ndescription: Unindexed note\ntype: reference\n---\nAlone.\n",
    );
    put(
        &dir.join("MEMORY.md"),
        "- [A](a.md) — first\n- [B](b.md) — second\n- [Ghost](ghost.md) — missing\n",
    );
    for name in ["nested.md", "flat.md"] {
        let body = fs::read(fixtures().join(name)).expect("fixture");
        put(&dir.join(format!("fx-{name}")), body);
    }
}

fn intl_vault(dir: &Path) {
    for name in ["Zeta", "e-plain", "é-accent", "ёlka", "ewe"] {
        put(
            &dir.join(format!("{name}.md")),
            format!("---\nname: {name}\n---\n[[e-plain]]\n"),
        );
    }
    put(
        &dir.join("MEMORY.md"),
        "- [Zeta](Zeta.md) — z\n- [[é-accent]]\n",
    );
    let rounded = dir.join("rounded.md");
    put(&rounded, "---\nname: rounded\n---\nx\n");
    pin(
        &rounded,
        SystemTime::UNIX_EPOCH + Duration::new(1_786_698_711, 122_999_906),
    );
}

/// `buildMemHome` at `out` (empty, made by the caller).
pub fn build_mem_home(out: &Path) {
    let projects = out.join(".claude/projects");
    proj_a(&projects.join("-Users-demo-proj-a/memory"));
    put(
        &projects.join("-Users-demo-proj-b/memory/MEMORY.md"),
        "# Notes\n\nSome prose.\n\n## More\n- bullet only\n",
    );
    let c = projects.join("-Users-demo-proj-c/memory");
    let lines: Vec<String> = (0..205)
        .map(|i| {
            if i == 203 {
                "- [Late](late.md) — below fold".to_owned()
            } else {
                format!("line {i}")
            }
        })
        .collect();
    put(&c.join("MEMORY.md"), format!("{}\n", lines.join("\n")));
    put(
        &c.join("late.md"),
        "---\nname: late\ndescription: d\ntype: project\n---\nbody\n",
    );
    put(
        &projects.join("-Users-demo-only-index/memory/MEMORY.md"),
        "",
    );
    fs::create_dir_all(projects.join("-Users-demo-empty/memory")).expect("mkdir");
    fs::create_dir_all(projects.join("-Users-demo-nomem")).expect("mkdir");
    put(&projects.join("-Users-demo-stray/memory/x.txt"), "x");
    put(
        &projects.join("C#proj/memory/hash.md"),
        "---\nname: hash\n---\nx\n",
    );
    put(
        &projects.join("C#proj/memory/MEMORY.md"),
        "- [Hash](hash.md)\n",
    );
    put(&projects.join("plainfile"), "not a dir");
    seed_vault(&projects.join(SEED).join("memory"));
    intl_vault(&projects.join("-Users-demo-intl/memory"));
    put(&out.join("outside/memory/x.md"), "---\nname: x\n---\nx\n");
    put(
        &out.join("work/projects/-work-proj/memory/w.md"),
        "---\nname: w\ntype: user\n---\nwork\n",
    );
    let work = out.join("work").to_string_lossy().into_owned();
    put(
        &out.join(".peektrace/settings.json"),
        serde_json::json!({"roots": {"claude": [{"path": work, "label": "work"}]}}).to_string(),
    );
}

/// One Claude session in the seed project, so the picker has a dated row.
pub fn seed_session(out: &Path) {
    let id = "77777777-7777-4777-8777-777777777777";
    let line = serde_json::json!({
        "type": "user", "cwd": "/Users/demo/seed", "sessionId": id,
        "timestamp": "2026-05-30T08:00:00.000Z",
        "message": {"role": "user", "content": "Remember the conventions."}
    });
    put(
        &out.join(format!(".claude/projects/{SEED}/{id}.jsonl")),
        format!("{line}\n"),
    );
}

/// Small vaults for the headless scenes: a seed vault under the parser
/// project and a vault-only project.
pub fn add_app_memory(home: &Path) {
    let projects = home.join(".claude/projects");
    seed_vault(&projects.join("-Users-demo-parser/memory"));
    put(
        &projects.join("-Users-demo-notes/memory/style.md"),
        "---\nname: style\ndescription: House style\ntype: feedback\n---\nShort sentences. See [[tone]].\n",
    );
}
