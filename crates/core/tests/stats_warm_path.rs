//! What a warm pass leans on (binaries lexed once at extract time, shipped
//! lines redacted once), plus markers, the sample pool cut, and facts that
//! ignore system-prompt events.
#![allow(clippy::float_cmp, reason = "seq numbers are whole")]

mod common;

use common::{bash_fact, bash_mut, bash_step, calls, facts_of, facts_of_text, step, transcript_of};
use peektrace_core::Agent;
use peektrace_core::js::time::Utc;
use peektrace_core::sessions::redact::redact_text;
use peektrace_core::stats::accumulate::{
    CorpusSummary, DrillTable, DrillTarget, FinishCtx, SampleRef, Tally,
};
use peektrace_core::stats::detectors::causes::spec;
use peektrace_core::stats::detectors::clustering::ClusterTable;
use peektrace_core::stats::detectors::failures::FailTable;
use peektrace_core::stats::detectors::timing::TimeTable;
use peektrace_core::stats::detectors::waiting::WaitPanelSink;
use peektrace_core::stats::markers::MarkerCollector;
use peektrace_core::stats::normalize::cluster_key_of;
use peektrace_core::stats::rank::pick_samples;
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{CallFact, CauseId, Fact, Job, Kind};
use peektrace_core::stats::shell_lex::ran_bins;
use serde_json::json;

fn secret() -> String {
    [
        "sk-ant-api03-",
        "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234",
    ]
    .concat()
}

#[test]
fn extract_stores_each_binary_the_command_runs_once() {
    let facts = facts_of(
        &[bash_step(
            "FOO=1 bun run build && git status | head -5; bun test",
            "ok",
            false,
        )],
        "sess-1",
    );
    let b = calls(&facts)[0].bash.as_deref().expect("bash");
    assert_eq!(b.ran, ["bun", "git", "head"]);
    assert_eq!(b.ran, ran_bins(&b.cmd));
}

fn shipped_key(sig: &str) -> String {
    Redactor::new(true).text(&cluster_key_of(sig, &|s: &str| s.to_owned()))
}

/// Every table that samples fact text, finished and drilled, as one string.
fn shipped(redact: &Redactor, facts: &[CallFact], drills: &[(DrillTable, String)]) -> String {
    let corpus = CorpusSummary::default();
    let window = common::window();
    let mut out = String::new();
    let targets: Vec<Option<DrillTarget>> = std::iter::once(None)
        .chain(drills.iter().map(|(table, key)| {
            Some(DrillTarget {
                table: *table,
                key: key.clone(),
            })
        }))
        .collect();
    for target in &targets {
        let mut time = TimeTable::new(target.as_ref());
        let mut wait = WaitPanelSink::new(target.as_ref());
        let mut fail = FailTable::new(target.clone());
        let mut cluster = ClusterTable::new(target.as_ref());
        for f in facts {
            let fact = Fact::Call(f.clone());
            time.add(&fact);
            wait.add(&fact);
            fail.add(&fact);
            cluster.add(&fact, redact);
        }
        let mut ctx = FinishCtx {
            corpus: &corpus,
            window: &window,
            redact,
            limit: 20.0,
            sample_cap: 3,
            caveats: Vec::new(),
        };
        out.push_str(&format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
            time.finish(&mut ctx),
            time.drill(&ctx),
            wait.finish(&ctx),
            wait.drill(&ctx),
            fail.finish(&mut ctx),
            fail.drill(&ctx),
            cluster.finish(&mut ctx),
            cluster.drill(&ctx),
        ));
    }
    out
}

fn stale_facts(cmd: &dyn Fn(u32) -> String, sig: &str) -> Vec<CallFact> {
    (0..3)
        .map(|seq| {
            let mut f = bash_fact(&cmd(seq));
            f.seq = f64::from(seq);
            f.err = true;
            f.failed = true;
            f.sig = Some(sig.to_owned());
            bash_mut(&mut f).job = Job::WaitPoll;
            f
        })
        .collect()
}

fn drills(sig: &str) -> Vec<(DrillTable, String)> {
    vec![
        (DrillTable::Time, "wait/poll".into()),
        (DrillTable::Wait, "all".into()),
        (DrillTable::Fail, "flag:Bash".into()),
        (DrillTable::Cluster, shipped_key(sig)),
    ]
}

#[test]
fn a_secret_a_shard_still_holds_never_ships() {
    let s = secret();
    assert!(!redact_text(&s).contains(&s));
    let sig = format!("request failed with key {s}");
    let facts = stale_facts(
        &|seq| format!("curl -H \"x-api-key: {s}\" https://example.test/{seq}"),
        &sig,
    );
    let out = shipped(&Redactor::new(true), &facts, &drills(&sig));
    assert!(out.contains("example.test"));
    assert!(out.contains("REDACTED"));
    assert!(!out.contains(&s));
}

#[test]
fn a_secret_straddling_the_sample_clip_ships_masked() {
    let s = secret();
    let filler = "x ".repeat(70);
    let sig = format!("{filler}{s}");
    let facts = stale_facts(&|_| format!("{filler}{s}"), &sig);
    let out = shipped(&Redactor::new(true), &facts, &drills(&sig));
    assert!(out.contains("REDACTED"));
    assert!(!out.contains(&s[..16]));
}

#[test]
fn redactor_work_follows_what_ships_not_the_fact_count() {
    const FACTS: u32 = 5000;
    let redact = Redactor::new(true);
    let facts: Vec<CallFact> = (0..FACTS)
        .map(|seq| {
            let mut f = bash_fact(&format!("bun run task-{seq}"));
            f.seq = f64::from(seq);
            f.err = true;
            f.failed = true;
            f.sig = Some("task failed".into());
            bash_mut(&mut f).job = if seq % 2 == 0 {
                Job::WaitPoll
            } else {
                Job::Build
            };
            f
        })
        .collect();
    shipped(&redact, &facts, &[]);
    let n = redact.memo_len();
    assert!(n > 0);
    assert!(n < (FACTS / 25) as usize, "{n}");
}

fn mark_of(cmd: &str, cause: CauseId, arg: Option<&str>) -> String {
    let redact = Redactor::new(true);
    let mut c = MarkerCollector::new("s1", &redact, &Utc);
    let mut f = bash_fact(cmd);
    f.tool_use_id = Some("t0".into());
    f.err = true;
    f.cause = Some(cause);
    f.cause_arg = arg.map(str::to_owned);
    c.add(&Fact::Call(f));
    c.finish()
        .into_iter()
        .next()
        .map(|m| m.label)
        .unwrap_or_default()
}

#[test]
fn a_long_fix_reaches_the_transcript_whole() {
    let label = mark_of("pkill -f srv", CauseId::PkillSelfKill, None);
    let fix = spec(CauseId::PkillSelfKill).fix.unwrap_or_default();
    assert!(fix.encode_utf16().count() > 160);
    assert!(label.ends_with(fix), "{label}");
}

#[test]
fn a_missing_binary_mark_names_its_own_fix() {
    assert_eq!(
        mark_of("jq . f", CauseId::CommandNotFound, Some("jq")),
        "command not found (jq): install jq"
    );
}

#[test]
fn batch_pool_cuts_keep_the_same_top_samples() {
    let mut x: u64 = 56;
    let mut next = move || {
        x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
        x
    };
    let mut tally = Tally::default();
    let mut eager: Vec<SampleRef> = Vec::new();
    for seq in 0..2000u32 {
        let mut fact = bash_fact("true");
        fact.seq = f64::from(seq);
        fact.base.sid = format!("s{}", next() % 7);
        let sample = SampleRef {
            sid: fact.base.sid.clone(),
            agent: Agent::Claude,
            project: "p".into(),
            kind: Kind::Main,
            parent: None,
            seq: f64::from(seq),
            tool_use_id: None,
            ts: None,
            line: "x".repeat(usize::try_from(next() % 9).unwrap_or(0)),
        };
        tally.add(&fact, Some(sample.clone()));
        eager.push(sample);
        if eager.len() > 64 {
            eager = pick_samples(&eager, 64);
        }
    }
    assert_eq!(pick_samples(&tally.samples, 3), pick_samples(&eager, 3));
    assert_eq!(pick_samples(&tally.samples, 64), pick_samples(&eager, 64));
}

fn snapshot(tools: &[&str]) -> String {
    json!({
        "type": "attachment",
        "timestamp": "2026-08-01T09:59:59.000Z",
        "cwd": "/work/proj",
        "attachment": {
            "type": "prompt_snapshot",
            "systemPrompt": ["You are a test agent."],
            "tools": tools.iter().map(|n| json!({ "name": n, "description": format!("Runs {n}.") })).collect::<Vec<_>>(),
        },
    })
    .to_string()
}

#[test]
fn facts_are_identical_with_and_without_system_prompt_events() {
    let base = transcript_of(&[
        bash_step("ls", "a\nb", false),
        bash_step("bun test", " 1 pass\n 2 fail\n", false),
        step("Read", json!({ "file_path": "x.ts" }), "", true),
    ]);
    let lines: Vec<&str> = base.trim_end().split('\n').collect();
    let mut all = vec![snapshot(&["Bash"])];
    all.extend(lines[..3].iter().map(|l| (*l).to_owned()));
    all.push(snapshot(&["Bash", "Read"]));
    all.extend(lines[3..].iter().map(|l| (*l).to_owned()));
    let text = all.join("\n") + "\n";
    let lean = facts_of_text(&text, "sess-1", true, true);
    assert_eq!(lean, facts_of_text(&text, "sess-1", true, false));
    let rows: Vec<&str> = lean
        .iter()
        .map(|f| match f {
            Fact::Session(_) => "session",
            Fact::Call(c) if c.is_bash() => "bash",
            Fact::Call(_) => "tool",
            Fact::Usage(_) => "usage",
        })
        .collect();
    assert_eq!(
        rows,
        ["session", "bash", "bash", "tool", "usage", "usage", "usage"]
    );
}
