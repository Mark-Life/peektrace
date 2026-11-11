//! One synthetic Claude transcript end to end: parse, extract, fold into the
//! cause and fail tables.
#![expect(clippy::float_cmp, reason = "seq and counts are whole numbers")]

use peektrace_core::Agent;
use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::stats::accumulate::{CorpusSummary, FinishCtx, StatsWindow};
use peektrace_core::stats::detectors::failures::FailTable;
use peektrace_core::stats::detectors::retries::CauseTable;
use peektrace_core::stats::extract::{ExtractInput, extract_facts};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CauseId, Fact, Kind};
use peektrace_core::stats::walk::TranscriptRef;

const SECRET: &str = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/core/test/fixtures/sessions-claude/retries.jsonl"
);

fn facts(enabled: bool) -> Vec<Fact> {
    let text = std::fs::read_to_string(PATH).unwrap_or_default();
    let parsed = parse_claude_session(&ParseArgs {
        text: &text,
        path: PATH,
        session_id: "retries",
        include_sidechain_turns: false,
        skip_system_prompt: true,
    });
    let r = TranscriptRef {
        id: "retries".into(),
        slug: "proj".into(),
        path: PATH.into(),
        source: None,
        agent: Agent::Claude,
        kind: Kind::Main,
        parent: None,
        run: None,
    };
    extract_facts(&ExtractInput {
        r#ref: &r,
        parsed: &parsed,
        project: "proj",
        redact: &Redactor::new(enabled),
        local: &Utc,
    })
}

struct Tables {
    causes: Vec<peektrace_core::stats::detectors::retries::CauseRow>,
    fails: Vec<peektrace_core::stats::detectors::failures::FailRow>,
}

fn tables(enabled: bool) -> Tables {
    let redact = Redactor::new(enabled);
    let mut causes = CauseTable::new(None, &Utc);
    let mut fails = FailTable::new(None);
    for f in facts(enabled) {
        causes.add(&f);
        fails.add(&f);
    }
    let corpus = CorpusSummary::default();
    let window = StatsWindow {
        days: 60.0,
        from_iso: String::new(),
        to_iso: String::new(),
    };
    let mut ctx = FinishCtx {
        corpus: &corpus,
        window: &window,
        redact: &redact,
        limit: 20.0,
        sample_cap: 3,
        caveats: Vec::new(),
    };
    Tables {
        causes: causes.finish(&mut ctx),
        fails: fails.finish(&mut ctx),
    }
}

#[test]
fn every_call_carries_its_request_and_context() {
    let all = facts(true);
    let calls: Vec<_> = all.iter().filter_map(Fact::as_call).collect();
    assert_eq!(calls.len(), 22);
    assert_eq!(calls[0].req.as_deref(), Some("req_0"));
    assert_eq!(calls[0].ctx_tok, Some(21_000.0));
    assert!(calls[4].side);
    assert_eq!(calls[4].ctx_tok, None);
    let Fact::Session(s) = &all[0] else {
        panic!("first fact is the session row");
    };
    assert_eq!(s.cwd.as_deref(), Some("/work/proj"));
}

#[test]
fn each_cause_lands_on_its_call() {
    use CauseId::*;
    let all = facts(true);
    let causes: Vec<Option<CauseId>> = all
        .iter()
        .filter_map(Fact::as_call)
        .map(|c| c.cause)
        .collect();
    let want = [
        Some(ZshGlobFlag),
        None,
        Some(ZshGlobPath),
        None,
        Some(ZshGlobFlag),
        None,
        None,
        Some(PkillSelfKill),
        None,
        Some(SleepBlocked),
        None,
        Some(CommandNotFound),
        None,
        Some(ReadBeforeWrite),
        None,
        None,
        Some(StructuredOutputSchema),
        None,
        Some(OutputTooLarge),
        None,
        Some(ZshGlobFlag),
        None,
    ];
    assert_eq!(causes, want);
    let first = all
        .iter()
        .find_map(Fact::as_call)
        .map(|c| (c.err, c.failed));
    assert_eq!(first, Some((false, true)));
}

#[test]
fn siblings_and_sidechains_never_pair() {
    let t = tables(true);
    let pairs: Vec<(&str, f64)> = t
        .causes
        .iter()
        .map(|r| (r.cause.as_str(), r.pairs))
        .collect();
    for (cause, n) in [
        ("zsh-glob-flag", 2.0),
        ("zsh-glob-path", 0.0),
        ("pkill-self-kill", 1.0),
        ("sleep-blocked", 1.0),
        ("command-not-found", 1.0),
        ("read-before-write", 1.0),
        ("structured-output-schema", 1.0),
        ("output-too-large", 1.0),
    ] {
        assert!(pairs.contains(&(cause, n)), "{cause} {n} in {pairs:?}");
    }
    assert_eq!(pairs.len(), 8);
    let glob = &t.causes[0];
    assert_eq!(glob.cause, CauseId::ZshGlobFlag);
    assert_eq!((glob.hits, glob.flagged), (3.0, 2.0));
    assert_eq!((glob.hits_by.main, glob.hits_by.nested), (2.0, 1.0));
    assert_eq!(
        glob.diffs,
        [
            (
                "--include=*.env".to_owned(),
                "--include='*.env'".to_owned(),
                1.0
            ),
            (
                "--include=*.ts".to_owned(),
                "--include='*.ts'".to_owned(),
                1.0
            ),
        ]
    );
    assert_eq!(glob.retry_ctx_tokens, 32_000.0);
}

#[test]
fn claimed_calls_stay_out_of_the_fail_table() {
    assert_eq!(tables(true).fails, []);
}

#[test]
fn no_secret_reaches_facts_or_rows() {
    let json: String = facts(true)
        .iter()
        .map(peektrace_core::stats::schema::fact_json)
        .collect();
    assert!(!json.contains(SECRET));
    let t = tables(true);
    let mut w = peektrace_core::js::json_out::Writer::new(false);
    w.begin_arr();
    for r in &t.causes {
        w.elem();
        r.write(&mut w);
    }
    w.end_arr();
    assert!(!w.finish().contains(SECRET));
}

#[test]
fn an_unredacted_pass_finds_the_same_causes() {
    let key = |t: Tables| -> Vec<(String, f64, f64)> {
        t.causes
            .into_iter()
            .map(|r| (r.key, r.hits, r.pairs))
            .collect()
    };
    assert_eq!(key(tables(false)), key(tables(true)));
}

#[test]
fn markers_name_the_cause_and_the_retry() {
    use peektrace_core::stats::markers::MarkerCollector;
    let redact = Redactor::new(true);
    let mut c = MarkerCollector::new("retries", &redact, &Utc);
    for f in facts(true) {
        c.add(&f);
    }
    let marks = c.finish();
    let at = |seq: f64| -> Vec<&str> {
        marks
            .iter()
            .filter(|m| m.seq == seq)
            .map(|m| m.detector.as_str())
            .collect()
    };
    assert_eq!(at(0.0), ["zsh-glob-flag"]);
    assert_eq!(at(1.0), ["retry"]);
    assert_eq!(
        marks
            .iter()
            .find(|m| m.seq == 1.0)
            .map(|m| m.label.as_str()),
        Some("retry of the Bash call 10s earlier: --include=*.ts → --include='*.ts'")
    );
    assert_eq!(at(19.0), ["retry"]);
}
