//! Per-cause state the cause table folds into, and the helpers it ranks with.

use std::cmp::Ordering;
use std::collections::HashSet;

use indexmap::IndexMap;

use super::cause_row::{CauseRow, PairSample};
use super::causes::{Per, glob_value, spec};
use super::cells::{CellSeries, sum_cells};
use super::rules::Split;
use crate::js::units;
use crate::stats::accumulate::SampleRef;
use crate::stats::rank::{cmp_str, compare_samples, desc};
use crate::stats::schema::{CallFact, CauseId};

const FAMILY_MAX: usize = 40;

#[derive(Default)]
pub(super) struct CauseAgg {
    pub(super) cause: Option<CauseId>,
    pub(super) ctx_seen: HashSet<String>,
    pub(super) details: IndexMap<String, f64>,
    pub(super) diffs: IndexMap<String, f64>,
    pub(super) flagged: f64,
    pub(super) git_diff_hits: f64,
    pub(super) hits_by_cell: CellSeries,
    pub(super) pair_samples: Vec<PairSample>,
    pub(super) pairs: f64,
    pub(super) pairs_by: Split,
    pub(super) retry_ctx_tokens: f64,
    pub(super) retry_sec: f64,
    pub(super) retry_sessions: HashSet<String>,
    pub(super) samples: Vec<SampleRef>,
    pub(super) sec_measured: f64,
    pub(super) sessions: HashSet<String>,
    pub(super) tokens_measured: f64,
    pub(super) tool: Option<String>,
}

impl CauseAgg {
    pub(super) fn cause(&self) -> CauseId {
        self.cause.unwrap_or(CauseId::Unclassified)
    }

    pub(super) fn hits(&self) -> Split {
        sum_cells([&self.hits_by_cell], &|_| true)
    }
}

pub(super) fn cause_key_of(cause: CauseId, tool: &str) -> String {
    if cause == CauseId::Unclassified {
        format!("cause:unclassified:{tool}")
    } else {
        format!("cause:{}", cause.as_str())
    }
}

pub(super) fn line_of(f: &CallFact) -> &str {
    match &f.bash {
        Some(b) => &b.cmd,
        None => f.sig.as_deref().or(f.arg.as_deref()).unwrap_or(&f.tool),
    }
}

pub(super) fn detail_of(cause: CauseId, f: &CallFact) -> Option<String> {
    if cause == CauseId::OutputTooLarge {
        return Some(match &f.bash {
            Some(b) => units::head(&b.family, FAMILY_MAX).into_owned(),
            None => f.tool.clone(),
        });
    }
    let arg = f.cause_arg.as_deref()?;
    Some(if cause == CauseId::ZshGlobFlag {
        glob_value(arg).to_owned()
    } else {
        arg.to_owned()
    })
}

pub(super) fn compare_pair_samples(a: &PairSample, b: &PairSample) -> Ordering {
    compare_samples(&a.failed, &b.failed)
}

pub(super) fn pick_pair_samples(pool: &[PairSample], cap: usize) -> Vec<PairSample> {
    let mut v = pool.to_vec();
    v.sort_by(compare_pair_samples);
    v.truncate(cap);
    v
}

pub(super) fn compare_cause_rows(a: &CauseRow, b: &CauseRow) -> Ordering {
    desc(a.retry_sessions, b.retry_sessions)
        .then_with(|| desc(a.sessions, b.sessions))
        .then_with(|| desc(a.pairs, b.pairs))
        .then_with(|| cmp_str(&a.key, &b.key))
}

#[derive(Default)]
pub(super) struct Denominators {
    pub(super) shell: CellSeries,
    pub(super) tools: IndexMap<String, CellSeries>,
}

impl Denominators {
    pub(super) fn of<'a>(&'a self, agg: &CauseAgg) -> Vec<&'a CellSeries> {
        let tools: Vec<&str> = match spec(agg.cause()).per {
            Per::Shell => return vec![&self.shell],
            Per::RowTool => agg.tool.as_deref().into_iter().collect(),
            Per::Tools(t) => t.to_vec(),
        };
        tools.iter().filter_map(|t| self.tools.get(*t)).collect()
    }
}
