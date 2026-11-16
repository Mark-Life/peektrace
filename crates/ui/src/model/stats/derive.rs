//! Core's text of a report for the desktop app, which builds it in process.

use peektrace_core::Agent;
use peektrace_core::js::time::LocalZone;
use peektrace_core::stats::service::StatsReport;
use peektrace_protocol::AgentId;

use super::StatsText;

pub const fn core_agent(a: AgentId) -> Agent {
    match a {
        AgentId::Claude => Agent::Claude,
        AgentId::Codex => Agent::Codex,
        AgentId::Pi => Agent::Pi,
        AgentId::Opencode => Agent::Opencode,
    }
}

/// Milliseconds east of UTC at the instant `ms`.
#[expect(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn offset_at(zone: &dyn LocalZone, ms: f64) -> f64 {
    let ms = ms as i64;
    let first = zone.offset_at_wall_ms(ms);
    zone.offset_at_wall_ms(ms + first) as f64
}

/// Everything core writes for `r`, scoped to `agent`.
pub fn text_of(r: &StatsReport, agent: Option<AgentId>) -> StatsText {
    peektrace_core::stats::text::text_of(r, agent.map(core_agent))
}
