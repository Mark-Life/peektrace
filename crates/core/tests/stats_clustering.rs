//! Error fingerprinting: one vocabulary, two entry points, six rules.
#![allow(clippy::float_cmp, reason = "counts are whole numbers")]

mod common;

use common::tool_fact;
use peektrace_core::js::units::len as len16;
use peektrace_core::stats::accumulate::{
    CorpusTotals, DrillHit, DrillTable, DrillTarget, FinishCtx, StatsWindow,
};
use peektrace_core::stats::detectors::clustering::{ClusterRow, ClusterTable, cluster_label};
use peektrace_core::stats::normalize::{cluster_key_of, error_signature, normalize_command};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, Fact};

fn sig_of(tool: &str, text: &str) -> Option<String> {
    error_signature(text, tool)
}

fn key_of(tool: &str, text: &str) -> String {
    cluster_key_of(&sig_of(tool, text).unwrap_or_default(), &|s: &str| {
        s.to_owned()
    })
}

fn fact_of(seq: usize, sid: &str, text: &str, tool: &str) -> CallFact {
    let mut f = tool_fact();
    sid.clone_into(&mut f.base.sid);
    "peektrace".clone_into(&mut f.base.project);
    f.seq = f64::from(u32::try_from(seq).unwrap_or(0));
    tool.clone_into(&mut f.tool);
    f.ts = Some(format!("2026-08-12T10:00:{seq:02}.000Z"));
    f.err = true;
    f.failed = true;
    f.sig = sig_of(tool, text);
    f.out_chars = f64::from(u32::try_from(len16(text)).unwrap_or(0));
    f
}

struct Clustered {
    rows: Vec<ClusterRow>,
    drill: Vec<DrillHit>,
}

fn cluster(rows: &[(&str, &str, &str)], drill: Option<&str>) -> Clustered {
    let redact = Redactor::new(true);
    let target = drill.map(|key| DrillTarget {
        table: DrillTable::Cluster,
        key: key.to_owned(),
    });
    let facts: Vec<Fact> = rows
        .iter()
        .enumerate()
        .map(|(seq, (tool, text, sid))| Fact::Call(fact_of(seq, sid, text, tool)))
        .collect();
    let mut table = ClusterTable::new(target.as_ref());
    let mut totals = CorpusTotals::default();
    for f in &facts {
        table.add(f, &redact);
        totals.add(f);
    }
    let corpus = totals.finish();
    let window = StatsWindow {
        days: 60.0,
        from_iso: "2026-06-13".into(),
        to_iso: "2026-08-12".into(),
    };
    let mut ctx = FinishCtx {
        corpus: &corpus,
        window: &window,
        redact: &redact,
        limit: 20.0,
        sample_cap: 3,
        caveats: Vec::new(),
    };
    Clustered {
        rows: table.finish(&mut ctx),
        drill: table.drill(&ctx),
    }
}

const SCHEMA_ONE: &str =
    "Output does not match required schema: root: must have required property 'verdict'";
const SCHEMA_FOUR: &str = "Output does not match required schema: root: must have required property 'angle', root: must have required property 'findings', root: must have required property 'method', root: must have required property 'nulls'";
const SCHEMA_MIXED: &str = "Output does not match required schema: root: must have required property 'refuted', root: must NOT have additional properties";
const SCHEMA_PATH: &str = "Output does not match required schema: /verdicts: must be array";

fn traceback(last: &str) -> String {
    format!(
        "Exit code 1\nrunning the probe\nTraceback (most recent call last):\n  File \"/tmp/p.py\", line 8, in <module>\n{last}"
    )
}

#[test]
fn fix1_shell_error_keyed_on_its_tail() {
    let a = key_of("Bash", &traceback("ValueError: EXA_API_KEY not found"));
    let b = key_of("Bash", &traceback("TypeError: not callable"));
    assert_ne!(a, b);
    assert_eq!(a, "Exit code 1 ValueError: EXA_API_KEY not found");
    assert_eq!(
        key_of("Bash", &traceback("KeyError: 'gong'")),
        key_of("Bash", "Exit code 1\nquerying\nKeyError: 'calls'")
    );
    assert_ne!(
        key_of("Bash", "Exit code 1\nno such thing"),
        key_of("Bash", "Exit code 143\nno such thing")
    );
    assert_eq!(
        key_of("Bash", "Exit code 143\nCommand timed out after 2m 0s"),
        "Exit code 143 Command timed out after <dur>"
    );
}

#[test]
fn fix2_repeated_clause_collapses_before_the_clip() {
    let t = cluster(
        &[
            ("StructuredOutput", SCHEMA_ONE, "s1"),
            ("StructuredOutput", SCHEMA_FOUR, "s2"),
            ("StructuredOutput", SCHEMA_MIXED, "s3"),
            ("StructuredOutput", SCHEMA_PATH, "s4"),
        ],
        None,
    );
    assert_eq!(t.rows.len(), 1);
    assert_eq!((t.rows[0].rows, t.rows[0].sessions), (4.0, 4.0));
    assert_eq!(t.rows[0].key, "Output does not match required schema:");
    assert_eq!(
        sig_of("StructuredOutput", SCHEMA_FOUR).as_deref(),
        Some("Output does not match required schema: root: must have required property 'angle'")
    );
}

#[test]
fn fix3_tool_use_error_cut_at_first_sentence() {
    let replace = |body: &str| {
        format!(
            "<tool_use_error>String to replace not found in file.\nString: {body}</tool_use_error>"
        )
    };
    assert_eq!(
        key_of("Edit", &replace("  --dry-run   Price the request.")),
        key_of("Edit", &replace("export const x = 1"))
    );
    assert_eq!(
        key_of("Edit", &replace("anything")),
        "String to replace not found in file."
    );
    let blocked = |n: u32, advice: &str| {
        format!(
            "<tool_use_error>Blocked: sleep {n} followed by: echo waited. {advice}</tool_use_error>"
        )
    };
    assert_eq!(
        key_of("Bash", &blocked(90, "Use Monitor with an until-loop.")),
        key_of("Bash", &blocked(45, "Use run_in_background instead."))
    );
    assert_eq!(
        key_of("Bash", &blocked(90, "anything")),
        "Blocked: sleep <n> followed by: echo waited."
    );
}

#[test]
fn fix4_durations_hosts_bins_and_plurals_are_masked() {
    assert_eq!(
        key_of("Bash", "Exit code 143\nCommand timed out after 2m 0s"),
        key_of("Bash", "Exit code 143\nCommand timed out after 10m 0s")
    );
    assert_eq!(
        key_of("WebFetch", "getaddrinfo ENOTFOUND search.marcia.dev"),
        "getaddrinfo ENOTFOUND <host>"
    );
    assert_eq!(
        key_of("WebFetch", "getaddrinfo ENOTFOUND search.marcia.dev"),
        key_of("WebFetch", "getaddrinfo ENOTFOUND search.marcia.cc")
    );
    assert!(!key_of("Bash", "Exit code 1\nBun v1.3.14 (macOS arm64)").contains("<host>"));
    let ls = "Exit code 1\nls: node_modules/x: No such file or directory";
    assert_eq!(
        key_of("Bash", ls),
        key_of(
            "Bash",
            "Exit code 1\ncat: node_modules/y: No such file or directory"
        )
    );
    assert_eq!(
        key_of("Bash", ls),
        "Exit code 1 <bin>: <path>: No such file or directory"
    );
    assert_eq!(
        key_of("Bash", "Exit code 1\nFound 1 issue"),
        key_of("Bash", "Exit code 1\nFound 3 issues")
    );
}

#[test]
fn fix5_the_fact_keeps_the_name_the_key_masks() {
    assert!(
        sig_of("StructuredOutput", SCHEMA_ONE)
            .unwrap_or_default()
            .contains("'verdict'")
    );
    let t = cluster(
        &[
            ("StructuredOutput", SCHEMA_ONE, "s1"),
            ("StructuredOutput", SCHEMA_ONE, "s2"),
            ("StructuredOutput", SCHEMA_FOUR, "s3"),
        ],
        None,
    );
    assert_eq!(
        t.rows[0].label,
        "Output does not match required schema — verdict, angle"
    );
    assert!(
        t.rows[0].samples[0]
            .line
            .contains("must have required property '")
    );
    let zod = "Exit code 1\nerror: Cannot find module 'zod'";
    assert_eq!(
        sig_of("Bash", zod).as_deref(),
        Some("Exit code 1 error: Cannot find module 'zod'")
    );
    assert_eq!(
        key_of("Bash", zod),
        "Exit code 1 error: Cannot find module <str>"
    );
}

#[test]
fn fix6_rank_by_rows_then_sessions() {
    let sids: Vec<String> = (0..5)
        .map(|i| format!("one-session-{}", i % 2))
        .chain((0..4).map(|i| format!("spread-{i}")))
        .collect();
    let rows: Vec<(&str, &str, &str)> = sids
        .iter()
        .enumerate()
        .map(|(i, sid)| {
            let text = if i < 5 {
                "Exit code 1\nboom"
            } else {
                "Exit code 1\nsplat"
            };
            ("Bash", text, sid.as_str())
        })
        .collect();
    let t = cluster(&rows, None);
    let got: Vec<(f64, f64)> = t.rows.iter().map(|r| (r.rows, r.sessions)).collect();
    assert_eq!(got, [(5.0, 2.0), (4.0, 4.0)]);
    let t = cluster(
        &[
            ("Bash", "Exit code 1\nboom", "a"),
            ("Bash", "Exit code 1\nboom", "a"),
            ("Bash", "Exit code 1\nsplat", "b"),
            ("Bash", "Exit code 1\nsplat", "c"),
        ],
        None,
    );
    assert_eq!(t.rows[0].key, "Exit code 1 splat");
}

const CORPUS: [(&str, &str, &str); 6] = [
    ("StructuredOutput", SCHEMA_ONE, "w1"),
    ("StructuredOutput", SCHEMA_FOUR, "w2"),
    ("StructuredOutput", SCHEMA_MIXED, "w3"),
    (
        "Bash",
        "Exit code 1\nls: a/b: No such file or directory",
        "b1",
    ),
    (
        "Bash",
        "Exit code 1\ncat: c/d: No such file or directory",
        "b2",
    ),
    (
        "Write",
        "<tool_use_error>File has not been read yet.</tool_use_error>",
        "b1",
    ),
];

#[test]
fn the_top_rows_reproduce() {
    let t = cluster(&CORPUS, None);
    let got: Vec<(&str, f64, f64, &str)> = t
        .rows
        .iter()
        .map(|r| (r.key.as_str(), r.rows, r.sessions, r.tool.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "Output does not match required schema:",
                3.0,
                3.0,
                "StructuredOutput"
            ),
            (
                "Exit code 1 <bin>: <path>: No such file or directory",
                2.0,
                2.0,
                "Bash"
            ),
            ("File has not been read yet.", 1.0, 1.0, "Write"),
        ]
    );
    assert_eq!(cluster(&CORPUS, None).rows, cluster(&CORPUS, None).rows);
}

#[test]
fn drill_returns_calls_in_call_order() {
    let t = cluster(&CORPUS, Some("Output does not match required schema:"));
    let sids: Vec<&str> = t.drill.iter().map(|h| h.sample.sid.as_str()).collect();
    assert_eq!(sids, ["w1", "w2", "w3"]);
    assert!(t.drill[0].sample.line.contains("'verdict'"));
    assert_eq!(cluster(&CORPUS, None).drill, []);
    assert_eq!(
        cluster_label("Output does not match required schema:"),
        "Output does not match required schema"
    );
}

fn secret() -> String {
    format!("ghp_{}", &"AbCdEfGh".repeat(5)[..36])
}

#[test]
fn a_secret_never_reaches_the_row_key() {
    let s = secret();
    let text = format!("Exit code 1\nauth failed for {s}");
    let leaky = [("Bash", text.as_str(), "r1"), ("Bash", text.as_str(), "r2")];
    let row = &cluster(&leaky, None).rows[0];
    assert!(!row.key.contains(&s) && !row.label.contains(&s));
    let hits = cluster(&leaky, Some(&row.key)).drill;
    let sids: Vec<&str> = hits.iter().map(|h| h.sample.sid.as_str()).collect();
    assert_eq!(sids, ["r1", "r2"]);
    assert!(!hits.iter().any(|h| h.sample.line.contains(&s)));
}

#[test]
fn a_secret_is_redacted_before_the_key_is_clipped() {
    let s = secret();
    let text = format!("Exit code 1\n{}{} {s}", "'' ".repeat(10), "x".repeat(99));
    let sig = sig_of("Bash", &text).unwrap_or_default();
    assert!(sig.contains(&s));
    assert!(!cluster_key_of(&sig, &|x: &str| x.to_owned()).contains(&s));
    let row = &cluster(&[("Bash", &text, "r1")], None).rows[0];
    assert!(!row.key.contains("ghp_"));
    let hits = cluster(&[("Bash", &text, "r1")], Some(&row.key)).drill;
    let sids: Vec<&str> = hits.iter().map(|h| h.sample.sid.as_str()).collect();
    assert_eq!(sids, ["r1"]);
}

#[test]
fn command_families() {
    let family = |s: &str| normalize_command(s).family;
    assert_eq!(
        family("bun run typecheck --filter web"),
        "bun run typecheck"
    );
    assert_eq!(
        family("bun run typecheck --filter api"),
        "bun run typecheck"
    );
    assert_eq!(family("bun test"), "bun test");
    assert_eq!(
        family("echo \"=== typecheck ===\"; bun run typecheck"),
        "bun run typecheck"
    );
    assert_eq!(family("cd /tmp && echo hi && git status"), "git status");
    assert_eq!(family("for f in *.ts; do wc -l $f; done"), "for-loop");
    assert_eq!(
        family("for P in $(lsof -ti:3000); do kill $P; done"),
        "for-loop"
    );
    assert_eq!(
        normalize_command("python3 - <<'PY'\nimport os\nprint(os.getcwd())\nPY").bin,
        "python3"
    );
    assert_eq!(family("   "), "");
    assert_eq!(family("weirdbin"), "weirdbin");
}
