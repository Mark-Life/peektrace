//! The cause table: hits per named cause, the retries each one cost, and
//! the counts the rule check reads back.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use super::cause_agg::{
    CauseAgg, Denominators, cause_key_of, compare_cause_rows, detail_of, line_of, pick_pair_samples,
};
pub use super::cause_row::{CauseRow, PairSample};
use super::causes::{GIT_DIFF_FIX, spec};
use super::cells::{
    Origin, add_to, bump_cell, command_not_found_fix, is_nested, reaches, sum_cells, top_counts,
};
use super::pairing::{Pairer, RetryPair, hit_cause_of};
use super::rules::{CauseCounts, CoverSince, Split, per1k_of};
use crate::js::time::{LocalZone, parse_date};
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, hit_of, redact_hits, sample_line,
    sample_of,
};
use crate::stats::rank::{SAMPLE_POOL, asc, bump, cmp_str, desc, pick_samples, rank, round};
use crate::stats::redact::Redactor;
use crate::stats::schema::{CallFact, CauseId, Fact};
use crate::stats::shell_lex::bin_name;

const DETAIL_CAP: usize = 5;
const DIFF_CAP: usize = 3;
pub const FIX_MAX: usize = 400;
const VALUE_MAX: usize = 80;
const RULE_DIRS: usize = 5;

/// The cause table, fed one fact at a time.
pub struct CauseTable<'a> {
    aggs: IndexMap<String, CauseAgg>,
    den: Denominators,
    session_origin: HashMap<String, Origin>,
    project_cwd: HashMap<String, String>,
    drill_hits: IndexMap<String, DrillHit>,
    drill_key: Option<String>,
    last_miss: HashMap<String, f64>,
    last_ran: HashMap<String, f64>,
    on_host: HashSet<String>,
    pairer: Pairer<'a>,
    flushed: bool,
    local: &'a dyn LocalZone,
}

impl<'a> CauseTable<'a> {
    pub fn new(drill: Option<&DrillTarget>, local: &'a dyn LocalZone) -> Self {
        Self {
            aggs: IndexMap::new(),
            den: Denominators::default(),
            session_origin: HashMap::new(),
            project_cwd: HashMap::new(),
            drill_hits: IndexMap::new(),
            drill_key: drill
                .filter(|d| d.table == DrillTable::Cause)
                .map(|d| d.key.clone()),
            last_miss: HashMap::new(),
            last_ran: HashMap::new(),
            on_host: HashSet::new(),
            pairer: Pairer::new(local),
            flushed: false,
            local,
        }
    }

    fn ran_since(&self, name: &str) -> bool {
        matches!((self.last_ran.get(name), self.last_miss.get(name)), (Some(r), Some(m)) if r > m)
    }

    fn installed(&self, bin: &str) -> bool {
        let name = bin_name(bin);
        self.ran_since(name) || self.on_host.contains(name)
    }

    #[expect(clippy::cast_precision_loss)]
    fn latest(at: &mut HashMap<String, f64>, bin: &str, f: &CallFact, local: &dyn LocalZone) {
        let Some(ms) = f.ts.as_deref().and_then(|t| parse_date(t, local)) else {
            return;
        };
        let ms = ms as f64;
        if at.get(bin).is_none_or(|&cur| ms > cur) {
            at.insert(bin.to_owned(), ms);
        }
    }

    fn agg_for(&mut self, cause: CauseId, tool: &str) -> &mut CauseAgg {
        let key = cause_key_of(cause, tool);
        self.aggs.entry(key).or_insert_with(|| CauseAgg {
            cause: Some(cause),
            tool: (cause == CauseId::Unclassified).then(|| tool.to_owned()),
            ..CauseAgg::default()
        })
    }

    fn on_pair(&mut self, pair: &RetryPair) {
        let drill_key = self.drill_key.clone();
        let (failed, retry) = (&pair.failed, &pair.retry);
        let agg = self.agg_for(pair.cause, &failed.tool);
        agg.pairs += 1.0;
        add_to(&mut agg.pairs_by, is_nested(failed));
        agg.retry_sessions.insert(failed.base.sid.clone());
        if let Some(g) = pair.gap_sec {
            agg.retry_sec += g;
            agg.sec_measured += 1.0;
        }
        if let Some(tok) = retry.ctx_tok {
            agg.tokens_measured += 1.0;
            let id = format!(
                "{}\0{}",
                retry.base.sid,
                retry
                    .req
                    .clone()
                    .unwrap_or_else(|| crate::js::num::to_js_string(retry.seq))
            );
            if agg.ctx_seen.insert(id) {
                agg.retry_ctx_tokens += tok;
            }
        }
        if let Some(d) = &pair.diff {
            bump(&mut agg.diffs, &format!("{}\0{}", d.from, d.to), 1.0);
        }
        let failed_line = sample_line(line_of(failed));
        let retry_line = sample_line(line_of(retry));
        agg.pair_samples.push(PairSample {
            failed: sample_of(failed, failed_line),
            retry: sample_of(retry, retry_line.clone()),
        });
        if agg.pair_samples.len() > SAMPLE_POOL * 2 {
            agg.pair_samples = pick_pair_samples(&agg.pair_samples, SAMPLE_POOL);
        }
        if drill_key.as_deref() == Some(cause_key_of(pair.cause, &failed.tool).as_str()) {
            let id = format!(
                "{}#{}",
                failed.base.sid,
                crate::js::num::to_js_string(failed.seq)
            );
            if let Some(hit) = self.drill_hits.get_mut(&id) {
                hit.pair_tool_use_id.clone_from(&retry.tool_use_id);
                hit.pair_line = Some(retry_line);
            }
        }
    }

    fn add_hit(&mut self, f: &CallFact, cause: CauseId, origin: &Origin) {
        let local = self.local;
        let drill = self.drill_key.as_deref() == Some(cause_key_of(cause, &f.tool).as_str());
        let detail = detail_of(cause, f);
        if let (Some(d), CauseId::CommandNotFound) = (&detail, cause) {
            Self::latest(&mut self.last_miss, bin_name(d), f, local);
        }
        let agg = self.agg_for(cause, &f.tool);
        if f.err {
            agg.flagged += 1.0;
        }
        bump_cell(&mut agg.hits_by_cell, f, origin);
        agg.sessions.insert(f.base.sid.clone());
        if let Some(d) = &detail {
            bump(&mut agg.details, d, 1.0);
        }
        if f.bash
            .as_ref()
            .is_some_and(|b| b.family.starts_with("git diff"))
        {
            agg.git_diff_hits += 1.0;
        }
        let line = sample_line(line_of(f));
        agg.samples.push(sample_of(f, line.clone()));
        crate::stats::rank::bound_pool(&mut agg.samples);
        if drill {
            let id = format!("{}#{}", f.base.sid, crate::js::num::to_js_string(f.seq));
            self.drill_hits.insert(id, hit_of(f, line));
        }
    }

    pub fn add(&mut self, fact: &Fact) {
        let f = match fact {
            Fact::Session(s) => {
                self.session_origin.insert(
                    s.base.sid.clone(),
                    Origin {
                        cwd: s.cwd.clone(),
                        source: s.source.clone(),
                    },
                );
                if let Some(cwd) = &s.cwd {
                    let better = self
                        .project_cwd
                        .get(&s.base.project)
                        .is_none_or(|k| cmp_str(cwd, k) == Ordering::Less);
                    if better {
                        self.project_cwd.insert(s.base.project.clone(), cwd.clone());
                    }
                }
                return;
            }
            Fact::Call(c) => c,
            Fact::Usage(_) => return,
        };
        let origin = self
            .session_origin
            .get(&f.base.sid)
            .cloned()
            .unwrap_or_default();
        if f.is_bash() {
            bump_cell(&mut self.den.shell, f, &origin);
        }
        bump_cell(
            self.den.tools.entry(f.tool.clone()).or_default(),
            f,
            &origin,
        );
        let cause = hit_cause_of(f);
        if let Some(c) = cause {
            self.add_hit(f, c, &origin);
        }
        if let Some(b) = &f.bash
            && !f.err
            && cause != Some(CauseId::CommandNotFound)
        {
            for bin in &b.ran {
                Self::latest(&mut self.last_ran, bin, f, self.local);
            }
        }
        for pair in self.pairer.step(f) {
            self.on_pair(&pair);
        }
    }

    fn settle(&mut self) {
        if !self.flushed {
            for pair in self.pairer.flush() {
                self.on_pair(&pair);
            }
            self.flushed = true;
        }
    }

    fn fix_of(&self, agg: &CauseAgg) -> Option<String> {
        let s = spec(agg.cause());
        match agg.cause() {
            CauseId::CommandNotFound => {
                let bins: Vec<String> = top_counts(&agg.details, agg.details.len())
                    .into_iter()
                    .map(|(b, _)| b)
                    .collect();
                let missing: Vec<String> = bins
                    .iter()
                    .filter(|b| !self.installed(b))
                    .take(4)
                    .cloned()
                    .collect();
                if !bins.is_empty() && missing.is_empty() {
                    return None;
                }
                command_not_found_fix(&missing).or(s.fix.map(str::to_owned))
            }
            CauseId::OutputTooLarge => {
                let hits = agg.hits();
                Some(if agg.git_diff_hits * 2.0 >= hits.total() {
                    GIT_DIFF_FIX.to_owned()
                } else {
                    s.fix.unwrap_or_default().to_owned()
                })
            }
            CauseId::StructuredOutputSchema => {
                let fix = s.fix.unwrap_or_default();
                Some(match top_counts(&agg.details, 1).first() {
                    Some((top, _)) => format!("{fix} Most often missing: '{top}'."),
                    None => fix.to_owned(),
                })
            }
            _ => s.fix.map(str::to_owned),
        }
    }

    #[expect(clippy::cast_precision_loss)]
    fn row_of(&self, ctx: &FinishCtx<'_>, key: &str, agg: &CauseAgg) -> CauseRow {
        let s = spec(agg.cause());
        let redact = ctx.redact;
        let hits_by = agg.hits();
        let projects: HashSet<&str> = agg
            .hits_by_cell
            .values()
            .map(|c| c.project.as_str())
            .collect();
        let re = |x: &SampleRef| SampleRef {
            line: redact.line(&x.line),
            ..x.clone()
        };
        CauseRow {
            key: key.to_owned(),
            cause: agg.cause(),
            kind: s.kind,
            label: redact.line(&agg.tool.as_ref().map_or_else(
                || s.label.to_owned(),
                |t| format!("{t} errored, no named cause"),
            )),
            fix: self.fix_of(agg).map(|f| redact.line_max(&f, FIX_MAX)),
            failing: s.failing,
            hits: hits_by.total(),
            flagged: agg.flagged,
            hits_by,
            pairs: agg.pairs,
            pairs_by: agg.pairs_by,
            retry_sessions: agg.retry_sessions.len() as f64,
            sessions: agg.sessions.len() as f64,
            projects: projects.len() as f64,
            retry_sec: round(agg.retry_sec),
            sec_measured: agg.sec_measured,
            retry_ctx_tokens: agg.retry_ctx_tokens,
            tokens_measured: agg.tokens_measured,
            per1k: per1k_of(hits_by, sum_cells(self.den.of(agg), &|_| true)),
            details: top_counts(&agg.details, DETAIL_CAP)
                .into_iter()
                .map(|(v, n)| (redact.line_max(&v, VALUE_MAX), n))
                .collect(),
            diffs: top_counts(&agg.diffs, DIFF_CAP)
                .into_iter()
                .map(|(pair, n)| {
                    let mut it = pair.split('\0');
                    let from = it.next().unwrap_or("");
                    let to = it.next().unwrap_or("");
                    (
                        redact.line_max(from, VALUE_MAX),
                        redact.line_max(to, VALUE_MAX),
                        n,
                    )
                })
                .collect(),
            rule: None,
            samples: pick_samples(&agg.samples, ctx.sample_cap)
                .iter()
                .map(re)
                .collect(),
            pair_samples: pick_pair_samples(&agg.pair_samples, ctx.sample_cap)
                .iter()
                .map(|p| PairSample {
                    failed: re(&p.failed),
                    retry: re(&p.retry),
                })
                .collect(),
        }
    }

    /// Ranked rows; the table stays readable for `since` afterwards.
    pub fn finish(&mut self, ctx: &mut FinishCtx<'_>) -> Vec<CauseRow> {
        self.settle();
        let rows: Vec<CauseRow> = self
            .aggs
            .iter()
            .filter(|(_, a)| a.cause() != CauseId::Unclassified || a.pairs > 0.0)
            .map(|(k, a)| self.row_of(ctx, k, a))
            .collect();
        let ranked = rank(rows, compare_cause_rows, ctx.limit);
        if ranked.dropped > 0 {
            ctx.caveat(format!("cause table: {} rows not shown", ranked.dropped));
        }
        ranked.rows
    }

    fn cwd_of(&self, cell: &super::cells::Cell) -> Option<String> {
        cell.cwd
            .clone()
            .or_else(|| self.project_cwd.get(&cell.project).cloned())
    }

    /// The working dirs each cause hit most in, sorted, unique.
    pub fn project_dirs(&self) -> Vec<String> {
        let mut dirs: HashSet<String> = HashSet::new();
        for agg in self
            .aggs
            .values()
            .filter(|a| a.cause() != CauseId::Unclassified)
        {
            let mut by_dir: IndexMap<String, f64> = IndexMap::new();
            for cell in agg.hits_by_cell.values() {
                if let Some(cwd) = self.cwd_of(cell) {
                    bump(&mut by_dir, &cwd, cell.main + cell.nested);
                }
            }
            dirs.extend(top_counts(&by_dir, RULE_DIRS).into_iter().map(|(d, _)| d));
        }
        let mut out: Vec<String> = dirs.into_iter().collect();
        out.sort_by(|a, b| cmp_str(a, b));
        out
    }

    pub fn drill(&mut self, redact: &Redactor) -> Vec<DrillHit> {
        self.settle();
        let hits: Vec<DrillHit> = self.drill_hits.values().cloned().collect();
        let mut hits = redact_hits(&hits, redact);
        hits.sort_by(|a, b| {
            asc(
                f64::from(u8::from(a.pair_line.is_none())),
                f64::from(u8::from(b.pair_line.is_none())),
            )
            .then_with(|| {
                cmp_str(
                    a.sample.ts.as_deref().unwrap_or(""),
                    b.sample.ts.as_deref().unwrap_or(""),
                )
            })
            .then_with(|| cmp_str(&a.sample.sid, &b.sample.sid))
            .then_with(|| asc(a.sample.seq, b.sample.seq))
        });
        hits
    }

    fn miss_count(&self, bin: &str) -> f64 {
        self.aggs.get("cause:command-not-found").map_or(0.0, |a| {
            a.details
                .iter()
                .filter(|(v, _)| bin_name(v) == bin)
                .map(|(_, n)| *n)
                .sum()
        })
    }

    /// Binaries a not-found named that no later call ran, most hits first.
    pub fn missing_bins(&self) -> Vec<String> {
        let mut bins: Vec<String> = self
            .last_miss
            .keys()
            .filter(|b| !self.ran_since(b))
            .cloned()
            .collect();
        bins.sort_by(|a, b| {
            desc(self.miss_count(a), self.miss_count(b)).then_with(|| cmp_str(a, b))
        });
        bins
    }

    pub fn mark_installed(&mut self, bins: &[String]) {
        self.on_host.extend(bins.iter().cloned());
    }
}

impl CauseCounts for CauseTable<'_> {
    fn since(&self, key: &str, covers: &[CoverSince<'_>]) -> (Split, Split) {
        let Some(agg) = self.aggs.get(key) else {
            return (Split::default(), Split::default());
        };
        let keep = |cell: &super::cells::Cell| {
            let cwd = self.cwd_of(cell);
            covers.iter().any(|c| {
                reaches(c.cover, cell, cwd.as_deref())
                    && (c.after_day.is_empty() || (!cell.day.is_empty() && cell.day > c.after_day))
            })
        };
        (
            sum_cells([&agg.hits_by_cell], &keep),
            sum_cells(self.den.of(agg), &keep),
        )
    }
}
