//! Cost findings: where the estimated spend goes, as cards with the main
//! sessions behind them and one change each.

use super::detectors::corpus::{ContextInsight, FanoutInsight, WebInsight};
use super::findings::{Finding, FindingFormat};
use super::rank::{desc, safe_div};
use super::service::StatsReport;

/// Share of the estimated spend at which a cost finding leads the list.
const LEAD_SHARE: f64 = 0.1;
/// Below this a dollar card says nothing worth a change.
const MIN_USD: f64 = 1.0;

/// `1 session`, `2 sessions`.
fn counted(n: f64, word: &str, f: &FindingFormat) -> String {
    let s = if n == 1.0 { "" } else { "s" };
    format!("{} {word}{s}", (f.count)(n))
}

fn dollars(usd: f64, f: &FindingFormat) -> String {
    format!("~${}", (f.count)(usd.round()))
}

fn card(key: &str, title: &str, impact: String, note: String, spread: String) -> Finding {
    Finding {
        detector: None,
        impact,
        key: key.to_owned(),
        note,
        spread,
        title: title.to_owned(),
        rule: None,
        table: None,
        samples: Vec::new(),
    }
}

fn fanout(i: &FanoutInsight, total: f64, f: &FindingFormat) -> Finding {
    let prefix = if i.prefix_tokens > 0.0 {
        format!(
            ": each workflow agent pays again for the prompt it starts with ({} tokens, {} in all)",
            (f.tokens)(i.prefix_tokens),
            dollars(i.prefix_usd, f)
        )
    } else {
        String::new()
    };
    Finding {
        samples: i.samples.clone(),
        ..card(
            "fanout",
            "Subagents and workflow agents",
            format!(
                "{} of {} ({})",
                dollars(i.usd, f),
                dollars(total, f),
                (f.pct)(safe_div(i.usd, total))
            ),
            format!("Spawn fewer agents per task, or give each a shorter prompt{prefix}."),
            format!(
                "{} from {}",
                counted(i.agents, "agent transcript", f),
                counted(i.parents, "session", f)
            ),
        )
    }
}

fn context(i: &ContextInsight, f: &FindingFormat) -> Finding {
    Finding {
        samples: i.samples.clone(),
        ..card(
            "context",
            if i.head == 1.0 {
                "One long session re-reads the most context"
            } else {
                "A few long sessions re-read the most context"
            },
            format!(
                "{} of cache reads in {}",
                dollars(i.usd, f),
                counted(i.head, "session", f)
            ),
            "Start a new session per task, or compact a long one, so each turn re-reads less."
                .to_owned(),
            format!(
                "{} of {} hold {} of their cache reads ({} of {} tokens)",
                (f.count)(i.head),
                counted(i.mains, "main session", f),
                (f.pct)(safe_div(i.head_read, i.total_read)),
                (f.tokens)(i.head_read),
                (f.tokens)(i.total_read)
            ),
        )
    }
}

fn web(i: &WebInsight, f: &FindingFormat) -> Finding {
    let impact = if i.in_run > 0.0 {
        format!("{} inside one run", counted(i.in_run, "repeat fetch", f))
    } else {
        format!("{} fetched more than once", counted(i.repeated, "url", f))
    };
    Finding {
        samples: i.samples.clone(),
        ..card(
            "web",
            "Web pages fetched again",
            impact,
            "Keep fetched pages for the run, or pass the first result to the agents that need it."
                .to_owned(),
            format!(
                "{} fetched more than once · {}",
                counted(i.repeated, "url or query", f),
                counted(i.calls, "web call", f)
            ),
        )
    }
}

/// Cost findings split into those that lead the list (a large share of the
/// spend, most dollars first) and the rest. Web re-fetches have no dollar
/// figure and always come last.
pub fn cost_findings(r: &StatsReport, f: &FindingFormat) -> (Vec<Finding>, Vec<Finding>) {
    let i = &r.panels.insights;
    let mut priced: Vec<(f64, Finding)> = i
        .fanout
        .iter()
        .map(|x| (x.usd, fanout(x, i.total_usd, f)))
        .chain(i.context.iter().map(|x| (x.usd, context(x, f))))
        .filter(|(usd, _)| *usd >= MIN_USD)
        .collect();
    priced.sort_by(|a, b| desc(a.0, b.0));
    let (lead, mut rest): (Vec<_>, Vec<_>) = priced
        .into_iter()
        .partition(|(usd, _)| safe_div(*usd, i.total_usd) >= LEAD_SHARE);
    let lead = lead.into_iter().map(|(_, x)| x).collect();
    rest.extend(i.web.iter().map(|x| (0.0, web(x, f))));
    (lead, rest.into_iter().map(|(_, x)| x).collect())
}
