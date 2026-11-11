//! Link Claude Code workflow runs to their subagents and to the Workflow
//! tool calls that started them.

use std::collections::HashMap;

use crate::js::time::{self, LocalZone};
use crate::js::value::JsValue;

const AGENT_PREFIX: &str = "agent-";
const RUN_ID_KEY: &str = "\"runId\"";

/// One agent of a workflow run, from `workflowProgress`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentPhase {
    pub phase_index: Option<f64>,
    pub phase_title: Option<String>,
    /// Epoch ms.
    pub started_at: Option<f64>,
}

/// A workflow run as read from `<session>/workflows/<runId>.json`, keyed by
/// stub id (`agent-<id>`), first entry per agent winning.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkflowRun {
    pub run_id: String,
    pub agents: HashMap<String, AgentPhase>,
}

/// A Workflow tool call that started or resumed a run.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowCall {
    /// Epoch ms of its tool result.
    pub at: Option<f64>,
    pub call_id: String,
}

/// Normalize an agent id to the stub form `agent-<id>`.
pub fn to_agent_ref(v: &str) -> String {
    if v.starts_with(AGENT_PREFIX) {
        v.to_owned()
    } else {
        format!("{AGENT_PREFIX}{v}")
    }
}

/// The agents of a run json. Bad JSON or missing keys give none.
pub fn parse_workflow_run(run_id: &str, raw: &str) -> WorkflowRun {
    let mut agents = HashMap::new();
    let doc = JsValue::parse(raw);
    let progress = doc
        .as_ref()
        .and_then(|d| d.as_obj())
        .and_then(|d| d.get("workflowProgress"))
        .and_then(JsValue::as_arr)
        .unwrap_or_default();
    for item in progress {
        let Some(o) = item.as_obj() else { continue };
        if o.get("type").and_then(JsValue::as_str) != Some("workflow_agent") {
            continue;
        }
        let Some(agent) = o.get("agentId").and_then(JsValue::as_str) else {
            continue;
        };
        agents
            .entry(to_agent_ref(agent))
            .or_insert_with(|| AgentPhase {
                phase_index: o.get("phaseIndex").and_then(JsValue::as_num),
                phase_title: o
                    .get("phaseTitle")
                    .and_then(JsValue::as_str)
                    .map(str::to_owned),
                started_at: o.get("startedAt").and_then(JsValue::as_num),
            });
    }
    WorkflowRun {
        run_id: run_id.to_owned(),
        agents,
    }
}

/// The `tool_use_id` of the first tool result block of a line.
fn tool_result_id(entry: &JsValue) -> Option<&str> {
    entry
        .get("message")?
        .as_obj()?
        .get("content")?
        .as_arr()?
        .iter()
        .find(|b| {
            b.as_obj().is_some_and(|b| {
                b.get("type").and_then(JsValue::as_str) == Some("tool_result")
                    && b.get("tool_use_id").and_then(JsValue::as_str).is_some()
            })
        })
        .and_then(|b| b.get("tool_use_id"))
        .and_then(JsValue::as_str)
}

/// Each workflow run id with the Workflow calls that started or resumed it,
/// in transcript order.
pub fn workflow_calls_by_run(
    text: &str,
    local: &dyn LocalZone,
) -> HashMap<String, Vec<WorkflowCall>> {
    let mut calls: HashMap<String, Vec<WorkflowCall>> = HashMap::new();
    for raw in text.split('\n').filter(|l| l.contains(RUN_ID_KEY)) {
        let Some(entry) = JsValue::parse(raw) else {
            continue;
        };
        let Some(result) = entry
            .as_obj()
            .and_then(|e| e.get("toolUseResult"))
            .and_then(JsValue::as_obj)
        else {
            continue;
        };
        let (Some(run_id), Some(call_id)) = (
            result.get("runId").and_then(JsValue::as_str),
            tool_result_id(&entry).filter(|c| !c.is_empty()),
        ) else {
            continue;
        };
        let list = calls.entry(run_id.to_owned()).or_default();
        if !list.iter().any(|c| c.call_id == call_id) {
            #[expect(clippy::cast_precision_loss, reason = "dates are within 8.64e15")]
            let at = entry
                .get("timestamp")
                .and_then(JsValue::as_str)
                .and_then(|t| time::parse_date(t, local))
                .map(|ms| ms as f64);
            list.push(WorkflowCall {
                at,
                call_id: call_id.to_owned(),
            });
        }
    }
    calls
}

/// The call an agent belongs to: the last one that returned before it
/// started, else the first.
pub fn pick_workflow_call(calls: &[WorkflowCall], started_at: Option<f64>) -> Option<&str> {
    let Some(start) = started_at else {
        return calls.first().map(|c| c.call_id.as_str());
    };
    calls
        .iter()
        .rfind(|c| c.at.is_some_and(|at| at <= start))
        .or_else(|| calls.first())
        .map(|c| c.call_id.as_str())
}
