//! Workflow runs and the subagents they spawn. Ported from `workflows` and
//! `workflow-subagents` TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::{TempDir, session_env, sources};
use peektrace_core::js::json_out::Writer;
use peektrace_core::js::time::{Utc, parse_date};
use peektrace_core::sessions::model::{ParsedSession, SubagentRef};
use peektrace_core::sessions::service::get_session;
use peektrace_core::sessions::workflows::{
    AgentPhase, WorkflowCall, parse_workflow_run, pick_workflow_call, to_agent_ref,
    workflow_calls_by_run,
};
use peektrace_core::sessions::write::write_subagent;
use serde_json::{Value, json};

const T1: &str = "2026-06-01T10:00:00.000Z";
const T2: &str = "2026-06-01T10:05:00.000Z";

#[expect(clippy::cast_precision_loss, reason = "dates fit f64")]
fn ms(iso: &str) -> f64 {
    parse_date(iso, &Utc).expect("date") as f64
}

fn result(call_id: &str, run_id: &Value, ts: Option<&str>) -> String {
    let mut o = json!({"type":"user","message":{"role":"user",
        "content":[{"type":"tool_result","tool_use_id":call_id,"content":"ok"}]},
        "toolUseResult":{"runId":run_id}});
    if let Some(ts) = ts {
        o["timestamp"] = json!(ts);
    }
    o.to_string()
}

fn call(id: &str, at: Option<f64>) -> WorkflowCall {
    WorkflowCall {
        at,
        call_id: id.to_owned(),
    }
}

#[test]
fn maps_a_run_id_to_its_call_and_result_time() {
    let calls = workflow_calls_by_run(&result("toolu_1", &json!("wf_a"), Some(T1)), &Utc);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls["wf_a"], [call("toolu_1", Some(ms(T1)))]);
}

#[test]
fn a_resumed_run_keeps_every_call_in_order() {
    let text = [
        result("toolu_1", &json!("wf_a"), Some(T1)),
        result("toolu_2", &json!("wf_a"), Some(T2)),
        result("toolu_2", &json!("wf_a"), Some(T2)),
    ]
    .join("\n");
    assert_eq!(
        workflow_calls_by_run(&text, &Utc)["wf_a"],
        [call("toolu_1", Some(ms(T1))), call("toolu_2", Some(ms(T2)))]
    );
}

#[test]
fn bad_workflow_lines_are_skipped() {
    let text = [
        r#"{"runId": broken"#.to_owned(),
        "not json".to_owned(),
        result("toolu_2", &json!(42), None),
        result("toolu_3", &json!("wf_b"), None),
    ]
    .join("\n");
    let calls = workflow_calls_by_run(&text, &Utc);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls["wf_b"], [call("toolu_3", None)]);
}

#[test]
fn run_id_lines_without_a_tool_result_are_ignored() {
    let text = [
        json!({"type":"user","toolUseResult":{"runId":"wf_a"}}),
        json!({"type":"assistant","message":{"content":[{"type":"text","text":"x"}]},
            "toolUseResult":{"runId":"wf_b"}}),
        json!({"type":"queue-operation","runId":"wf_c"}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    assert!(workflow_calls_by_run(&text, &Utc).is_empty());
}

#[test]
fn picks_the_last_call_before_the_agent_started() {
    let calls = [call("start", Some(ms(T1))), call("resume", Some(ms(T2)))];
    assert_eq!(
        pick_workflow_call(&calls, Some(ms(T1) + 1.0)),
        Some("start")
    );
    assert_eq!(
        pick_workflow_call(&calls, Some(ms(T2) + 1.0)),
        Some("resume")
    );
}

#[test]
fn pick_falls_back_to_the_first_call() {
    let calls = [call("start", Some(ms(T1))), call("resume", Some(ms(T2)))];
    assert_eq!(pick_workflow_call(&calls, None), Some("start"));
    assert_eq!(
        pick_workflow_call(&calls, Some(ms(T1) - 1.0)),
        Some("start")
    );
    assert_eq!(
        pick_workflow_call(&[call("only", None)], Some(ms(T2))),
        Some("only")
    );
    assert_eq!(pick_workflow_call(&[], Some(ms(T2))), None);
}

#[test]
fn reads_per_agent_phases_from_workflow_agent_entries() {
    let raw = json!({"workflowProgress":[
        {"type":"workflow_phase","index":0,"title":"survey"},
        {"type":"workflow_agent","agentId":"a1","phaseIndex":0,"phaseTitle":"survey","startedAt":1000},
        {"type":"workflow_agent","agentId":"a2"},
        {"type":"workflow_agent","phaseIndex":1}
    ]});
    let run = parse_workflow_run("wf", &raw.to_string());
    assert_eq!(run.agents.len(), 2);
    assert_eq!(
        run.agents["agent-a1"],
        AgentPhase {
            phase_index: Some(0.0),
            phase_title: Some("survey".to_owned()),
            started_at: Some(1000.0),
        }
    );
    assert_eq!(run.agents["agent-a2"], AgentPhase::default());
}

#[test]
fn bad_run_json_gives_no_agents() {
    for raw in ["{nope", "[]", "{}"] {
        assert!(parse_workflow_run("wf", raw).agents.is_empty(), "{raw}");
    }
}

#[test]
fn to_agent_ref_adds_the_prefix_once() {
    assert_eq!(to_agent_ref("plain"), "agent-plain");
    assert_eq!(to_agent_ref("agent-plain"), "agent-plain");
}

// workflow-subagents: one workflow (started, then resumed), one plain Task
// subagent that spawned a depth-2 subagent, an orphan run dir and a run
// whose phase holds a fake secret.

const SLUG: &str = "-Users-demo-wf";
const SID: &str = "22222222-2222-4222-8222-222222222222";
const RUN: &str = "wf_run-1";
const RUN_NO_JSON: &str = "wf_run-2";
const RUN_SECRET: &str = "wf_run-3";
const WF_CALL: &str = "toolu_workflow_1";
const WF_RESUME_CALL: &str = "toolu_workflow_resume";
const RESULT_AT: &str = "2026-06-01T10:00:02.000Z";
const RESUME_AT: &str = "2026-06-01T10:00:10.000Z";
const TASK_CALL: &str = "toolu_task_1";
const DEEP_CALL: &str = "toolu_task_deep";

fn line(o: &Value) -> String {
    format!("{o}\n")
}

fn usage() -> Value {
    json!({"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":5})
}

fn assistant(uuid: &str, req: &str, content: &Value) -> String {
    line(&json!({"type":"assistant","uuid":uuid,"sessionId":SID,
        "timestamp":"2026-06-01T10:00:01.000Z","requestId":req,
        "message":{"id":format!("msg-{uuid}"),"role":"assistant","model":"claude-opus-4",
            "content":content,"usage":usage()}}))
}

fn tool_result(uuid: &str, id: &str, text: &str, tur: Option<Value>, ts: &str) -> String {
    let mut o = json!({"type":"user","uuid":uuid,"sessionId":SID,"timestamp":ts,
        "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":text}]}});
    if let Some(t) = tur {
        o["toolUseResult"] = t;
    }
    line(&o)
}

fn main_transcript() -> String {
    let started = |task: &str| {
        Some(json!({"runId":RUN,"taskId":task,"status":"started","workflowName":"demo"}))
    };
    [
        line(
            &json!({"type":"user","uuid":"u0","sessionId":SID,"cwd":"/Users/demo/wf",
            "timestamp":"2026-06-01T10:00:00.000Z",
            "message":{"role":"user","content":"run the demo workflow"}}),
        ),
        assistant(
            "a1",
            "req-1",
            &json!([{"type":"tool_use","id":WF_CALL,"name":"Workflow","input":{"script":"x"}}]),
        ),
        tool_result(
            "u1",
            WF_CALL,
            &format!("Workflow started: {RUN}"),
            started("task-1"),
            RESULT_AT,
        ),
        assistant(
            "a2",
            "req-2",
            &json!([{"type":"tool_use","id":TASK_CALL,"name":"Task",
                "input":{"description":"plain task","prompt":"p"}}]),
        ),
        tool_result("u2", TASK_CALL, "done", None, RESULT_AT),
        assistant(
            "a3",
            "req-3",
            &json!([{"type":"tool_use","id":WF_RESUME_CALL,"name":"Workflow",
                "input":{"resumeFromRunId":RUN,"scriptPath":"x"}}]),
        ),
        tool_result(
            "u3",
            WF_RESUME_CALL,
            &format!("Workflow resumed: {RUN}"),
            started("task-2"),
            RESUME_AT,
        ),
    ]
    .concat()
}

fn sub_transcript(sid: &str, at: [&str; 2]) -> String {
    [
        line(
            &json!({"type":"user","uuid":format!("{sid}-u"),"isSidechain":true,"sessionId":SID,
            "timestamp":format!("2026-06-01T{}.000Z", at[0]),
            "message":{"role":"user","content":"go"}}),
        ),
        line(
            &json!({"type":"assistant","uuid":format!("{sid}-a"),"isSidechain":true,
            "sessionId":SID,"timestamp":format!("2026-06-01T{}.000Z", at[1]),
            "requestId":format!("req-{sid}"),
            "message":{"id":format!("msg-{sid}"),"role":"assistant","model":"claude-opus-4",
                "content":[{"type":"text","text":"ok"}],"usage":usage()}}),
        ),
    ]
    .concat()
}

fn layout() -> TempDir {
    let root = TempDir::new("wf-subagents");
    let sess = format!("projects/{SLUG}/{SID}");
    let sub = format!("{sess}/subagents");
    let run_dir = format!("{sub}/workflows/{RUN}");
    root.write(&format!("projects/{SLUG}/{SID}.jsonl"), &main_transcript());
    let agent = |n: usize, id: &str, phase: Option<(f64, &str)>, started: Option<f64>| {
        let mut a =
            json!({"type":"workflow_agent","index":n,"agentId":id,"label":id,"state":"done"});
        if let Some((i, t)) = phase {
            a["phaseIndex"] = json!(i);
            a["phaseTitle"] = json!(t);
        }
        if let Some(s) = started {
            a["startedAt"] = json!(s);
        }
        a
    };
    let run = json!({"runId":RUN,"taskId":"task-1","workflowName":"demo","status":"completed",
    "phases":[{"title":"survey"},{"title":"build"}],
    "workflowProgress":[
        {"type":"workflow_phase","index":0,"title":"survey"},
        agent(0, "wfa", Some((0.0, "survey")), Some(ms(RESULT_AT) + 1000.0)),
        {"type":"workflow_phase","index":1,"title":"build"},
        agent(1, "wfb", Some((1.0, "build")), Some(ms(RESUME_AT) + 1000.0)),
        agent(2, "wfc", Some((1.0, "build")), None),
    ]});
    root.write(&format!("{sess}/workflows/{RUN}.json"), &run.to_string());
    let write = |dir: &str, id: &str, meta: &Value, at: [&str; 2]| {
        root.write(&format!("{dir}/agent-{id}.jsonl"), &sub_transcript(id, at));
        root.write(&format!("{dir}/agent-{id}.meta.json"), &meta.to_string());
    };
    let at = ["10:00:03", "10:00:04"];
    write(
        &sub,
        "plain",
        &json!({"agentType":"general-purpose","description":"plain task","toolUseId":TASK_CALL,"spawnDepth":1}),
        at,
    );
    write(
        &sub,
        "deep",
        &json!({"agentType":"general-purpose","description":"nested task","toolUseId":DEEP_CALL,
            "parentAgentId":"plain","spawnDepth":2}),
        at,
    );
    let wf_meta = |desc: &str, phase: &Value| json!({"agentType":"workflow-agent","description":desc,"workflowPhase":phase,"spawnDepth":1});
    write(&run_dir, "wfa", &wf_meta("survey-a", &json!("survey")), at);
    write(&run_dir, "wfb", &wf_meta("build-b", &json!("build")), at);
    write(&run_dir, "wfc", &wf_meta("build-c", &Value::Null), at);
    write(
        &run_dir,
        "wfd",
        &wf_meta("build-d", &json!("build")),
        ["10:00:20", "10:00:21"],
    );
    root.write(&format!("{run_dir}/journal.jsonl"), "");
    write(
        &format!("{sub}/workflows/{RUN_NO_JSON}"),
        "orphan",
        &json!({"agentType":"workflow-agent","spawnDepth":1}),
        at,
    );
    root.write(
        &format!("{sess}/workflows/{RUN_SECRET}.json"),
        &json!({"runId":RUN_SECRET,"workflowProgress":[{"type":"workflow_agent","agentId":"secret",
            "phaseIndex":0,"phaseTitle":"unused"}]})
        .to_string(),
    );
    let fake_key = format!("sk-ant-{}", "a".repeat(48));
    write(
        &format!("{sub}/workflows/{RUN_SECRET}"),
        "secret",
        &json!({"agentType":"workflow-agent","workflowPhase":format!("phase {fake_key}"),"spawnDepth":1}),
        at,
    );
    root
}

fn parse(redact: bool) -> ParsedSession {
    let root = layout();
    let claude = sources(
        &format!("{}/home", root.str()),
        &format!("{}/projects", root.str()),
    );
    get_session(session_env(&claude), SID, None, redact).expect("session")
}

fn by_id<'a>(p: &'a ParsedSession, id: &str) -> &'a SubagentRef {
    p.subagents
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("no {id}"))
}

fn sub_json(s: &SubagentRef) -> Value {
    let mut w = Writer::new(false);
    write_subagent(&mut w, s);
    serde_json::from_str(&w.finish()).expect("json")
}

#[expect(clippy::unnecessary_wraps, reason = "builds expected Option fields")]
fn some(s: &str) -> Option<String> {
    Some(s.to_owned())
}

fn phase(p: &ParsedSession, id: &str) -> (Option<String>, Option<String>, Option<f64>) {
    let s = by_id(p, id);
    (
        s.workflow_run_id.clone(),
        s.workflow_phase.clone(),
        s.workflow_phase_index,
    )
}

#[test]
fn every_agent_file_is_found_including_workflow_runs() {
    let p = parse(false);
    let mut ids: Vec<_> = p.subagents.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "agent-deep",
            "agent-orphan",
            "agent-plain",
            "agent-secret",
            "agent-wfa",
            "agent-wfb",
            "agent-wfc",
            "agent-wfd"
        ]
    );
}

#[test]
fn a_workflow_agent_links_to_the_launching_call() {
    let p = parse(false);
    assert_eq!(by_id(&p, "agent-wfa").tool_use_id.as_deref(), Some(WF_CALL));
}

#[test]
fn an_agent_started_after_a_resume_links_to_the_resume_call() {
    let p = parse(false);
    assert_eq!(
        by_id(&p, "agent-wfb").tool_use_id.as_deref(),
        Some(WF_RESUME_CALL)
    );
}

#[test]
fn an_agent_missing_from_the_run_json_uses_its_transcript_start() {
    let p = parse(false);
    assert_eq!(
        by_id(&p, "agent-wfd").tool_use_id.as_deref(),
        Some(WF_RESUME_CALL)
    );
}

#[test]
fn the_internal_start_time_does_not_reach_the_output() {
    let p = parse(false);
    assert_eq!(
        sub_json(by_id(&p, "agent-wfa")).get("workflowStartedAt"),
        None
    );
}

#[test]
fn a_workflow_agent_carries_its_run_id_and_phase() {
    let p = parse(false);
    assert_eq!(
        phase(&p, "agent-wfa"),
        (some(RUN), some("survey"), Some(0.0))
    );
    assert_eq!(
        phase(&p, "agent-wfb"),
        (some(RUN), some("build"), Some(1.0))
    );
}

#[test]
fn a_depth_two_subagent_names_its_spawner() {
    let p = parse(false);
    assert_eq!(
        by_id(&p, "agent-deep").parent_agent_id.as_deref(),
        Some("agent-plain")
    );
    assert_eq!(by_id(&p, "agent-plain").parent_agent_id, None);
}

#[test]
fn a_null_meta_phase_falls_back_to_the_run_json() {
    let p = parse(false);
    assert_eq!(
        phase(&p, "agent-wfc"),
        (some(RUN), some("build"), Some(1.0))
    );
    assert_eq!(by_id(&p, "agent-wfc").tool_use_id.as_deref(), Some(WF_CALL));
}

#[test]
fn a_run_dir_without_run_json_keeps_its_id_and_stays_unlinked() {
    let p = parse(false);
    let orphan = by_id(&p, "agent-orphan");
    assert_eq!(orphan.workflow_run_id.as_deref(), Some(RUN_NO_JSON));
    assert_eq!(orphan.tool_use_id, None);
    assert_eq!(orphan.workflow_phase, None);
}

#[test]
fn a_run_without_a_workflow_result_stays_unlinked() {
    let p = parse(false);
    let secret = by_id(&p, "agent-secret");
    assert_eq!(secret.workflow_run_id.as_deref(), Some(RUN_SECRET));
    assert_eq!(secret.tool_use_id, None);
}

#[test]
fn non_workflow_agents_gain_no_workflow_keys() {
    let p = parse(false);
    let plain = sub_json(by_id(&p, "agent-plain"));
    let keys: Vec<_> = plain
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| k.starts_with("workflow"))
        .collect();
    assert_eq!(keys, Vec::<&String>::new());
}

#[test]
fn redact_covers_the_workflow_phase() {
    let p = parse(true);
    assert_eq!(
        by_id(&p, "agent-secret").workflow_phase.as_deref(),
        Some("phase [REDACTED:anthropic-key]")
    );
}
