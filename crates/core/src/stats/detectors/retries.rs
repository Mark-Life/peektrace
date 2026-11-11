//! The cause table: hits per named cause, the retries each one cost, and
//! the counts the rule check reads back.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use super::causes::{CauseKind, GIT_DIFF_FIX, Per, glob_value, spec};
use super::cells::{
    CellSeries, Origin, add_to, bump_cell, command_not_found_fix, is_nested, reaches, sum_cells,
    top_counts,
};
use super::pairing::{Pairer, RetryPair, hit_cause_of};
use super::rules::{CauseCounts, CoverSince, RuleCheck, Split, per1k_of};
use crate::js::json_out::Writer;
use crate::js::time::{LocalZone, parse_date};
use crate::js::units;
use crate::stats::accumulate::{
    DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef, hit_of, redact_hits, sample_line,
    sample_of, write_samples,
};
use crate::stats::rank::{
    SAMPLE_POOL, asc, bump, cmp_str, compare_samples, desc, pick_samples, rank, round,
};
use crate::stats::redact::Redactor;
use crate::stats::schema::{CallFact, CauseId, Fact, WriteExt};
use crate::stats::shell_lex::bin_name;

const DETAIL_CAP: usize = 5;
const DIFF_CAP: usize = 3;
pub const FIX_MAX: usize = 400;
const VALUE_MAX: usize = 80;
const FAMILY_MAX: usize = 40;
const RULE_DIRS: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct PairSample {
    pub failed: SampleRef,
    pub retry: SampleRef,
}

/// One row of the cause table.
#[derive(Debug, Clone, PartialEq)]
pub struct CauseRow {
    pub key: String,
    pub cause: CauseId,
    pub kind: CauseKind,
    pub label: String,
    pub fix: Option<String>,
    pub failing: bool,
    pub hits: f64,
    pub flagged: f64,
    pub hits_by: Split,
    pub pairs: f64,
    pub pairs_by: Split,
    pub retry_sessions: f64,
    pub sessions: f64,
    pub projects: f64,
    pub retry_sec: f64,
    pub sec_measured: f64,
    pub retry_ctx_tokens: f64,
    pub tokens_measured: f64,
    pub per1k: Split,
    pub details: Vec<(String, f64)>,
    pub diffs: Vec<(String, String, f64)>,
    pub rule: Option<RuleCheck>,
    pub samples: Vec<SampleRef>,
    pub pair_samples: Vec<PairSample>,
}

impl CauseRow {
    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("key").text(&self.key);
        w.key("cause").str(self.cause.as_str());
        w.key("kind").str(self.kind.as_str());
        w.key("label").text(&self.label);
        w.key("fix").opt_text(self.fix.as_deref());
        w.key("failing").bool(self.failing);
        w.key("hits").num(self.hits);
        w.key("flagged").num(self.flagged);
        w.key("hitsBy");
        self.hits_by.write(w);
        w.key("pairs").num(self.pairs);
        w.key("pairsBy");
        self.pairs_by.write(w);
        for (k, v) in [
            ("retrySessions", self.retry_sessions),
            ("sessions", self.sessions),
            ("projects", self.projects),
            ("retrySec", self.retry_sec),
            ("secMeasured", self.sec_measured),
            ("retryCtxTokens", self.retry_ctx_tokens),
            ("tokensMeasured", self.tokens_measured),
        ] {
            w.key(k).num(v);
        }
        w.key("per1k");
        self.per1k.write(w);
        w.key("details").begin_arr();
        for (value, hits) in &self.details {
            w.elem().begin_obj();
            w.key("value").text(value);
            w.key("hits").num(*hits);
            w.end_obj();
        }
        w.end_arr();
        w.key("diffs").begin_arr();
        for (from, to, pairs) in &self.diffs {
            w.elem().begin_obj();
            w.key("from").text(from);
            w.key("to").text(to);
            w.key("pairs").num(*pairs);
            w.end_obj();
        }
        w.end_arr();
        match &self.rule {
            Some(r) => {
                w.key("rule");
                r.write(w);
            }
            None => w.key("rule").null(),
        }
        w.key("samples");
        write_samples(w, &self.samples);
        w.key("pairSamples").begin_arr();
        for p in &self.pair_samples {
            w.elem().begin_obj();
            w.key("failed");
            p.failed.write(w);
            w.key("retry");
            p.retry.write(w);
            w.end_obj();
        }
        w.end_arr();
        w.end_obj();
    }
}

#[derive(Default)]
struct CauseAgg {
    cause: Option<CauseId>,
    ctx_seen: HashSet<String>,
    details: IndexMap<String, f64>,
    diffs: IndexMap<String, f64>,
    flagged: f64,
    git_diff_hits: f64,
    hits_by_cell: CellSeries,
    pair_samples: Vec<PairSample>,
    pairs: f64,
    pairs_by: Split,
    retry_ctx_tokens: f64,
    retry_sec: f64,
    retry_sessions: HashSet<String>,
    samples: Vec<SampleRef>,
    sec_measured: f64,
    sessions: HashSet<String>,
    tokens_measured: f64,
    tool: Option<String>,
}

impl CauseAgg {
    fn cause(&self) -> CauseId {
        self.cause.unwrap_or(CauseId::Unclassified)
    }

    fn hits(&self) -> Split {
        sum_cells([&self.hits_by_cell], &|_| true)
    }
}

fn cause_key_of(cause: CauseId, tool: &str) -> String {
    if cause == CauseId::Unclassified {
        format!("cause:unclassified:{tool}")
    } else {
        format!("cause:{}", cause.as_str())
    }
}

fn line_of(f: &CallFact) -> &str {
    match &f.bash {
        Some(b) => &b.cmd,
        None => f.sig.as_deref().or(f.arg.as_deref()).unwrap_or(&f.tool),
    }
}

fn detail_of(cause: CauseId, f: &CallFact) -> Option<String> {
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

fn compare_pair_samples(a: &PairSample, b: &PairSample) -> Ordering {
    compare_samples(&a.failed, &b.failed)
}

fn pick_pair_samples(pool: &[PairSample], cap: usize) -> Vec<PairSample> {
    let mut v = pool.to_vec();
    v.sort_by(compare_pair_samples);
    v.truncate(cap);
    v
}

fn compare_cause_rows(a: &CauseRow, b: &CauseRow) -> Ordering {
    desc(a.retry_sessions, b.retry_sessions)
        .then_with(|| desc(a.sessions, b.sessions))
        .then_with(|| desc(a.pairs, b.pairs))
        .then_with(|| cmp_str(&a.key, &b.key))
}

#[derive(Default)]
struct Denominators {
    shell: CellSeries,
    tools: IndexMap<String, CellSeries>,
}

impl Denominators {
    fn of<'a>(&'a self, agg: &CauseAgg) -> Vec<&'a CellSeries> {
        let tools: Vec<&str> = match spec(agg.cause()).per {
            Per::Shell => return vec![&self.shell],
            Per::RowTool => agg.tool.as_deref().into_iter().collect(),
            Per::Tools(t) => t.to_vec(),
        };
        tools.iter().filter_map(|t| self.tools.get(*t)).collect()
    }
}

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
        let Some(ms) = f.ts.as_deref().and_then(|t| parse_date(t, local)) else { return };
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

    fn on_pair(&mut self, pair: RetryPair) {
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
                retry.req.clone().unwrap_or_else(|| crate::js::num::to_js_string(retry.seq))
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
            let id = format!("{}#{}", failed.base.sid, crate::js::num::to_js_string(failed.seq));
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
        if f.bash.as_ref().is_some_and(|b| b.family.starts_with("git diff")) {
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
        let origin = self.session_origin.get(&f.base.sid).cloned().unwrap_or_default();
        if f.is_bash() {
            bump_cell(&mut self.den.shell, f, &origin);
        }
        bump_cell(self.den.tools.entry(f.tool.clone()).or_default(), f, &origin);
        let cause = hit_cause_of(f);
        if let Some(c) = cause {
            self.add_hit(f, c, &origin);
        }
        if let Some(b) = &f.bash {
            if !f.err && cause != Some(CauseId::CommandNotFound) {
                for bin in &b.ran {
                    Self::latest(&mut self.last_ran, bin, f, self.local);
                }
            }
        }
        for pair in self.pairer.step(f) {
            self.on_pair(pair);
        }
    }

    fn settle(&mut self) {
        if !self.flushed {
            for pair in self.pairer.flush() {
                self.on_pair(pair);
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
                let missing: Vec<String> = bins.iter().filter(|b| !self.installed(b)).take(4).cloned().collect();
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
        let projects: HashSet<&str> = agg.hits_by_cell.values().map(|c| c.project.as_str()).collect();
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
                    (redact.line_max(from, VALUE_MAX), redact.line_max(to, VALUE_MAX), n)
                })
                .collect(),
            rule: None,
            samples: pick_samples(&agg.samples, ctx.sample_cap).iter().map(re).collect(),
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
        cell.cwd.clone().or_else(|| self.project_cwd.get(&cell.project).cloned())
    }

    /// The working dirs each cause hit most in, sorted, unique.
    pub fn project_dirs(&self) -> Vec<String> {
        let mut dirs: HashSet<String> = HashSet::new();
        for agg in self.aggs.values().filter(|a| a.cause() != CauseId::Unclassified) {
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
            asc(f64::from(u8::from(a.pair_line.is_none())), f64::from(u8::from(b.pair_line.is_none())))
                .then_with(|| cmp_str(a.sample.ts.as_deref().unwrap_or(""), b.sample.ts.as_deref().unwrap_or("")))
                .then_with(|| cmp_str(&a.sample.sid, &b.sample.sid))
                .then_with(|| asc(a.sample.seq, b.sample.seq))
        });
        hits
    }

    fn miss_count(&self, bin: &str) -> f64 {
        self.aggs
            .get("cause:command-not-found")
            .map_or(0.0, |a| {
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
        bins.sort_by(|a, b| desc(self.miss_count(a), self.miss_count(b)).then_with(|| cmp_str(a, b)));
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
