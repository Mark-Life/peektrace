//! Session chips: what a link back from a stats row to its sessions says,
//! and how samples of one session fold into one chip.

use std::collections::HashMap;

use peektrace_js::text::trim;
use peektrace_js::units::collapse_ws;
use peektrace_protocol::AgentId;
use peektrace_protocol::stats::{DrillHitView, SampleRefView};

use crate::model::utf16;

/// UTF-16 units of a project slug a chip keeps.
pub const PROJECT_TAIL: usize = 18;
/// UTF-16 units of a session id a chip shows.
const SID_HEAD: usize = 8;
/// Code points of a title a chip shows before it is cut.
const TITLE_MAX: usize = 40;

/// Session titles by [`title_key`], as the session list knows them.
pub type SessionTitles = HashMap<String, String>;

pub fn title_key(agent: AgentId, id: &str) -> String {
    format!("{}:{id}", agent.as_str())
}

/// The end of a long project slug, cut after a `-` so a chip shows the
/// folder name, not the shared `-Users-<name>-` head.
pub fn project_tail(project: &str, max: usize) -> String {
    let len = utf16::len(project);
    if len <= max {
        return project.to_owned();
    }
    let tail = utf16::slice(project, len - max, len);
    let clean = utf16::slice(project, len - max - 1, len - max) == "-";
    let units: Vec<u16> = tail.encode_utf16().collect();
    let dash = units.iter().position(|&u| u == u16::from(b'-'));
    let cut = match dash {
        Some(d) if !clean && d != units.len() - 1 => utf16::slice(&tail, d + 1, units.len()),
        _ => tail,
    };
    format!("…{cut}")
}

/// What a sample's chip says: the session title, else `project/id`. `id`
/// is the short session id beside a title, so equal titles stay apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipLabel {
    pub project: String,
    pub session: String,
    pub subagent: bool,
    pub id: Option<String>,
    pub label: String,
}

/// The session a sample names: its parent, for a subagent call.
pub fn session_of(s: &SampleRefView) -> &str {
    s.parent.as_deref().unwrap_or(&s.sid)
}

fn clip(title: &str) -> String {
    if title.chars().count() > TITLE_MAX {
        let mut out: String = title.chars().take(TITLE_MAX - 1).collect();
        out.push('…');
        out
    } else {
        title.to_owned()
    }
}

fn clean_title(title: Option<&String>) -> Option<String> {
    let flat = collapse_ws(title?);
    let flat = trim(&flat);
    (!flat.is_empty()).then(|| flat.to_owned())
}

pub fn chip_label_of(s: &SampleRefView, titles: &SessionTitles) -> ChipLabel {
    let session = utf16::slice(session_of(s), 0, SID_HEAD);
    let project = project_tail(&s.project, PROJECT_TAIL);
    let title = clean_title(titles.get(&title_key(s.agent, session_of(s))));
    let label = title
        .as_deref()
        .map_or_else(|| format!("{project}/{session}"), clip);
    ChipLabel {
        id: title.map(|_| session.clone()),
        label,
        project,
        session,
        subagent: s.parent.is_some(),
    }
}

/// Samples folded by the session they name, in first-seen order.
pub fn session_chips(samples: &[SampleRefView]) -> Vec<Vec<&SampleRefView>> {
    let mut groups: Vec<(String, Vec<&SampleRefView>)> = Vec::new();
    for s in samples {
        let key = title_key(s.agent, session_of(s));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(s),
            None => groups.push((key, vec![s])),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// A chip's hover text: every sample behind it, one line each.
pub fn title_of(samples: &[&SampleRefView]) -> String {
    samples
        .iter()
        .map(|s| match &s.parent {
            None => format!("{} · {} · {}", s.project, s.sid, s.line),
            Some(p) => format!("{} · {p} · subagent {} · {}", s.project, s.sid, s.line),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One target in a folded chip's list.
pub fn target_label(s: &SampleRefView) -> String {
    let who = if s.parent.is_none() {
        "main".to_owned()
    } else {
        format!("subagent {}", s.sid)
    };
    format!("{who} · {}", s.line)
}

/// A drill hit's session: the title with project and short id, else
/// `project/id`.
pub fn drill_label(hit: &DrillHitView, titles: &SessionTitles) -> String {
    let chip = chip_label_of(&hit.sample, titles);
    chip.id.map_or_else(
        || chip.label.clone(),
        |id| format!("{} · {}/{id}", chip.label, chip.project),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample(sid: &str, parent: Option<&str>, project: &str) -> SampleRefView {
        SampleRefView {
            sid: sid.into(),
            agent: AgentId::Claude,
            project: project.into(),
            kind: if parent.is_some() { "subagent" } else { "main" }.into(),
            parent: parent.map(Into::into),
            seq: 1.0,
            tool_use_id: Some(format!("toolu_{sid}")),
            ts: None,
            line: format!("ls {sid}"),
        }
    }

    #[test]
    fn project_tail_cuts_at_a_dash() {
        assert_eq!(project_tail("-work-edge", 18), "-work-edge");
        assert_eq!(
            project_tail("-Users-demo-home-work-agents", 18),
            "…home-work-agents"
        );
        assert_eq!(
            project_tail("abcdefghijklmnopqrstuvwxyz", 18),
            "…ijklmnopqrstuvwxyz"
        );
        assert_eq!(
            project_tail("-Users-demo-x-abcdefghijklmnopq-", 18),
            "…abcdefghijklmnopq-"
        );
        assert_eq!(
            project_tail("aaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbbb", 18),
            "…bbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn a_title_names_the_chip_and_the_id_stays_beside_it() {
        let s = sample("22222222-aaaa", None, "-work-edge");
        let mut titles = SessionTitles::new();
        let none = chip_label_of(&s, &titles);
        assert_eq!(none.label, "-work-edge/22222222");
        assert_eq!(none.id, None);
        titles.insert(
            "claude:22222222-aaaa".into(),
            "  Edge parent\n session with a fairly long title that goes on ".into(),
        );
        let named = chip_label_of(&s, &titles);
        assert_eq!(named.label, "Edge parent session with a fairly long …");
        assert_eq!(named.id.as_deref(), Some("22222222"));
        titles.insert("claude:22222222-aaaa".into(), " \u{a0}\t".into());
        assert_eq!(chip_label_of(&s, &titles).id, None);
    }

    #[test]
    fn a_clip_never_splits_a_pair() {
        let title = format!("{}😀😀😀", "a".repeat(38));
        assert_eq!(clip(&title), format!("{}😀…", "a".repeat(38)));
    }

    #[test]
    fn subagent_samples_fold_under_their_parent() {
        let samples = [
            sample("agent-1", Some("p1"), "x"),
            sample("m1", None, "x"),
            sample("agent-2", Some("p1"), "x"),
            SampleRefView {
                agent: AgentId::Codex,
                ..sample("m1", None, "x")
            },
        ];
        let groups = session_chips(&samples);
        let sids: Vec<Vec<&str>> = groups
            .iter()
            .map(|g| g.iter().map(|s| s.sid.as_str()).collect())
            .collect();
        assert_eq!(sids, [vec!["agent-1", "agent-2"], vec!["m1"], vec!["m1"]]);
        assert_eq!(
            title_of(&groups[0]),
            "x · p1 · subagent agent-1 · ls agent-1\nx · p1 · subagent agent-2 · ls agent-2"
        );
        assert_eq!(target_label(groups[0][0]), "subagent agent-1 · ls agent-1");
        assert_eq!(target_label(groups[1][0]), "main · ls m1");
        assert_eq!(
            chip_label_of(groups[0][0], &SessionTitles::new()).session,
            "p1"
        );
    }
}
