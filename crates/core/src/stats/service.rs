//! The stats entry points: report, drill and refresh.

use std::collections::HashMap;

use thiserror::Error;
use tracing::info_span;

use super::accumulate::{
    CorpusSummary, CorpusTotals, DrillHit, DrillTable, DrillTarget, FinishCtx, SampleRef,
    StatsWindow,
};
use super::cache::StatsCache;
use super::detectors::clustering::{ClusterRow, ClusterTable};
use super::detectors::corpus::{CorpusPanelSink, CorpusPanels};
use super::detectors::failures::{FailRow, FailTable};
use super::detectors::pairing::{MAX_RETRY_GAP_SEC, PAIR_THRESHOLD, RETRY_WINDOW};
use super::detectors::retries::{CauseRow, CauseTable};
use super::detectors::rules::{RowRef, check_row};
use super::detectors::timing::{TimeRow, TimeTable};
use super::detectors::waiting::{WaitPanel, WaitPanelSink};
use super::host_bins::host_bins_of;
use super::memory_files::{AgentHomes, read_memory_docs};
use super::pass::{PassArgs, PassEnv, PassResult, is_full_scope, run_pass, scope_of};
use super::rank::{DEFAULT_DRILL_PAGE, DEFAULT_SAMPLE_CAP, uniq_sorted};
use super::redact::Redactor;
use super::rule_dates::date_lines;
use super::schema::{DETECTOR_VERSION, Fact, STATS_SCHEMA_VERSION};
use crate::js::json_out::Writer;
use crate::js::num::to_js_string;
use crate::js::time::{LocalZone, parse_date, to_iso_string};
use crate::model::Agent;
use crate::roots::Source;

pub const DEFAULT_DAYS: f64 = 60.0;
const MS_PER_DAY: f64 = 86_400_000.0;
const ALL_DAYS: f64 = 36_500.0;
/// Largest `|ms|` a JS `Date` holds.
const MAX_TIME: f64 = 8.64e15;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StatsError {
    #[error("{0}")]
    Scan(String),
    #[error("No {table} row with key \"{key}\". Run `peektrace stats` and copy a DRILL cell.")]
    RowNotFound { table: &'static str, key: String },
}

/// Everything a stats call reads from the environment.
pub struct StatsEnv<'a> {
    pub claude: &'a [Source],
    pub codex_home: String,
    pub pi_home: String,
    pub cache: StatsCache,
    pub local: &'a (dyn LocalZone + Sync),
    pub now_ms: f64,
}

impl StatsEnv<'_> {
    fn pass_env(&self) -> PassEnv<'_> {
        PassEnv {
            claude: self.claude,
            cache: &self.cache,
            local: self.local,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReportRequest {
    pub agents: Option<Vec<Agent>>,
    pub days: Option<f64>,
    pub force: bool,
    pub limit: Option<f64>,
}

/// `{days, fromIso, toIso}` for a window ending now; an error when the start
/// is not a valid date.
pub fn window_of(now: f64, days: f64) -> Result<StatsWindow, StatsError> {
    let from = now - days * MS_PER_DAY;
    if !from.is_finite() || from.abs() > MAX_TIME {
        return Err(StatsError::Scan(format!(
            "--days {} reaches past the earliest date",
            to_js_string(days)
        )));
    }
    #[expect(clippy::cast_possible_truncation)]
    Ok(StatsWindow {
        days,
        from_iso: to_iso_string(from as i64),
        to_iso: to_iso_string(now as i64),
    })
}

#[expect(clippy::cast_precision_loss)]
fn window_ms(w: &StatsWindow, local: &dyn LocalZone) -> (f64, f64) {
    let p = |s: &str| parse_date(s, local).map_or(f64::NAN, |m| m as f64);
    (p(&w.from_iso), p(&w.to_iso))
}

const BASE_CAVEATS: [&str; 7] = [
    "durSec is the gap between a call and its result: it includes harness overhead and approval waits",
    "silent-failure counts are a floor: a detector only sees the output the transcript stored",
    "backgrounded runs are charged the seconds they took, which understates the wait they started",
    "shell causes cover Claude, OpenCode and Pi shell rows; Codex exec_command output is not parsed for causes",
    "",
    "subagent and workflow-agent transcripts count their own model calls, so their tokens and cost are measured, not zero",
    "a rule match is a keyword probe over CLAUDE.md/AGENTS.md: it means a line mentions the habit, not that it states the fix",
];

fn base_caveats() -> Vec<String> {
    BASE_CAVEATS
        .iter()
        .map(|c| {
            if c.is_empty() {
                format!(
                    "a retry pair is a failed call followed within {} calls by a similar (hybrid similarity >= {}) succeeding call of the same tool, about 87% precise on a hand-labelled sample; retry seconds are call-to-call gaps of at most {}s",
                    to_js_string(RETRY_WINDOW),
                    to_js_string(PAIR_THRESHOLD),
                    to_js_string(MAX_RETRY_GAP_SEC)
                )
            } else {
                (*c).to_owned()
            }
        })
        .collect()
}

/// Every report sink, fed in this order.
struct Detectors<'a> {
    corpus: CorpusTotals,
    causes: CauseTable<'a>,
    fails: FailTable,
    time: TimeTable,
    waiting: WaitPanelSink,
    clusters: ClusterTable,
    panels: CorpusPanelSink,
    redact: Redactor,
}

impl<'a> Detectors<'a> {
    fn new(drill: Option<&DrillTarget>, local: &'a dyn LocalZone) -> Self {
        Self {
            corpus: CorpusTotals::default(),
            causes: CauseTable::new(drill, local),
            fails: FailTable::new(drill.cloned()),
            time: TimeTable::new(drill),
            waiting: WaitPanelSink::new(drill),
            clusters: ClusterTable::new(drill),
            panels: CorpusPanelSink::default(),
            redact: Redactor::new(true),
        }
    }

    fn add(&mut self, f: &Fact) {
        self.corpus.add(f);
        self.causes.add(f);
        self.fails.add(f);
        self.time.add(f);
        self.waiting.add(f);
        self.clusters.add(f, &self.redact);
        self.panels.add(f);
    }
}

struct Prepared<'a> {
    detectors: Detectors<'a>,
    result: PassResult,
    scope: Vec<Agent>,
    window: StatsWindow,
    limit: f64,
}

fn prepare<'a>(
    env: &'a StatsEnv<'a>,
    req: &ReportRequest,
    drill: Option<&DrillTarget>,
) -> Result<Prepared<'a>, StatsError> {
    let window = window_of(env.now_ms, req.days.unwrap_or(DEFAULT_DAYS))?;
    let (from_ms, to_ms) = window_ms(&window, env.local);
    let scope = scope_of(req.agents.as_deref());
    let mut detectors = Detectors::new(drill, env.local);
    let result = run_pass(
        &env.pass_env(),
        &PassArgs {
            scope: &scope,
            force: req.force,
            from_ms,
            to_ms,
            only: None,
        },
        &mut |f| detectors.add(f),
    );
    Ok(Prepared {
        detectors,
        result,
        scope,
        window,
        limit: req.limit.unwrap_or(super::rank::DEFAULT_ROW_LIMIT),
    })
}

/// The versioned report.
#[derive(Debug, Clone)]
pub struct StatsReport {
    pub generated_at: f64,
    pub window: StatsWindow,
    pub corpus: CorpusSummary,
    pub causes: Vec<CauseRow>,
    pub fails: Vec<FailRow>,
    pub time: Vec<TimeRow>,
    pub waiting: WaitPanel,
    pub clusters: Vec<ClusterRow>,
    pub panels: CorpusPanels,
    pub skipped: Vec<String>,
    pub caveats: Vec<String>,
    pub paths: Vec<(String, String)>,
}

fn paths_of<'s>(
    paths: &HashMap<String, String>,
    redact: &Redactor,
    sids: impl IntoIterator<Item = &'s str>,
) -> Vec<(String, String)> {
    uniq_sorted(sids)
        .into_iter()
        .filter_map(|sid| paths.get(&sid).map(|p| (sid.clone(), redact.text(p))))
        .collect()
}

fn samples_in(r: &StatsReport) -> Vec<&SampleRef> {
    let mut out: Vec<&SampleRef> = Vec::new();
    for row in &r.causes {
        out.extend(&row.samples);
        for p in &row.pair_samples {
            out.push(&p.failed);
            out.push(&p.retry);
        }
    }
    out.extend(r.fails.iter().flat_map(|x| &x.samples));
    out.extend(r.time.iter().flat_map(|x| &x.samples));
    out.extend(&r.waiting.samples);
    out.extend(r.clusters.iter().flat_map(|x| &x.samples));
    out
}

fn window_json(w: &mut Writer, win: &StatsWindow) {
    w.begin_obj();
    w.key("days").num(win.days);
    w.key("fromIso").str(&win.from_iso);
    w.key("toIso").str(&win.to_iso);
    w.end_obj();
}

fn paths_json(w: &mut Writer, paths: &[(String, String)]) {
    w.begin_obj();
    for (sid, p) in paths {
        w.key_text(sid).text(p);
    }
    w.end_obj();
}

impl StatsReport {
    pub fn to_json(&self, pretty: bool) -> String {
        let mut w = Writer::new(pretty);
        w.begin_obj();
        w.key("schemaVersion").str(STATS_SCHEMA_VERSION);
        w.key("generatedAt").num(self.generated_at);
        w.key("window");
        window_json(&mut w, &self.window);
        w.key("detectorVersion").num(DETECTOR_VERSION);
        w.key("corpus");
        self.corpus.write(&mut w);
        macro_rules! rows {
            ($k:literal, $v:expr) => {
                w.key($k).begin_arr();
                for r in $v {
                    w.elem();
                    r.write(&mut w);
                }
                w.end_arr();
            };
        }
        rows!("causes", &self.causes);
        rows!("fails", &self.fails);
        rows!("time", &self.time);
        w.key("waiting");
        self.waiting.write(&mut w);
        rows!("clusters", &self.clusters);
        w.key("context");
        self.panels.write_context(&mut w);
        w.key("fanout");
        self.panels.write_fanout(&mut w);
        w.key("web");
        self.panels.write_web(&mut w);
        w.key("cost");
        self.panels.write_cost(&mut w);
        w.key("skipped")
            .texts(self.skipped.iter().map(String::as_str));
        w.key("caveats")
            .texts(self.caveats.iter().map(String::as_str));
        w.key("paths");
        paths_json(&mut w, &self.paths);
        w.end_obj();
        w.finish()
    }
}

/// The report: one pass, then memory files, then every table.
pub fn report(env: &StatsEnv<'_>, req: &ReportRequest) -> Result<StatsReport, StatsError> {
    let _span = info_span!("Stats.report").entered();
    let mut p = prepare(env, req, None)?;
    let homes = AgentHomes {
        claude: env.claude,
        codex_home: env.codex_home.clone(),
        pi_home: env.pi_home.clone(),
    };
    let docs = read_memory_docs(&homes, &p.scope, &p.detectors.causes.project_dirs());
    let (dates, next) = date_lines(&docs, &env.cache.load_lines(), env.now_ms);
    env.cache.commit_lines(&next);
    let bins = host_bins_of(&p.detectors.causes.missing_bins());
    p.detectors.causes.mark_installed(&bins);
    let corpus = std::mem::take(&mut p.detectors.corpus).finish();
    let redact = Redactor::new(true);
    let mut ctx = FinishCtx {
        corpus: &corpus,
        window: &p.window,
        redact: &redact,
        limit: p.limit,
        sample_cap: DEFAULT_SAMPLE_CAP,
        caveats: base_caveats(),
    };
    let d = &mut p.detectors;
    let panels = d.panels.finish(&mut ctx);
    let mut causes = d.causes.finish(&mut ctx);
    for row in &mut causes {
        let rr = RowRef {
            key: &row.key,
            cause: row.cause,
            fix: row.fix.as_deref(),
            details: &row.details,
        };
        row.rule = check_row(&rr, &docs, &d.causes, &dates, &redact);
    }
    let pairs: f64 = causes.iter().map(|r| r.pairs).sum();
    let costed: f64 = causes.iter().map(|r| r.tokens_measured).sum();
    if pairs > costed {
        ctx.caveat(format!(
            "{} of {} retry pairs are uncosted in tokens: sidechain calls and calls with no recorded model usage carry no context count",
            to_js_string(pairs - costed),
            to_js_string(pairs)
        ));
    }
    let fails = d.fails.finish(&mut ctx);
    let time = d.time.finish(&mut ctx);
    let waiting = d.waiting.finish(&ctx);
    let clusters = d.clusters.finish(&mut ctx);
    let mut out = StatsReport {
        generated_at: env.now_ms,
        window: p.window.clone(),
        corpus: corpus.clone(),
        causes,
        fails,
        time,
        waiting,
        clusters,
        panels,
        skipped: p.result.skipped.clone(),
        caveats: std::mem::take(&mut ctx.caveats),
        paths: Vec::new(),
    };
    let sids: Vec<String> = samples_in(&out).iter().map(|s| s.sid.clone()).collect();
    out.paths = paths_of(&p.result.paths, &redact, sids.iter().map(String::as_str));
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct DrillResult {
    pub table: DrillTable,
    pub key: String,
    pub total: usize,
    pub offset: usize,
    pub hits: Vec<DrillHit>,
    pub paths: Vec<(String, String)>,
}

impl DrillResult {
    pub fn to_json(&self, pretty: bool) -> String {
        let mut w = Writer::new(pretty);
        w.begin_obj();
        w.key("schemaVersion").str(STATS_SCHEMA_VERSION);
        w.key("table").str(self.table.as_str());
        w.key("key").text(&self.key);
        #[expect(clippy::cast_possible_wrap)]
        {
            w.key("total").int(self.total as i64);
            w.key("offset").int(self.offset as i64);
        }
        w.key("hits").begin_arr();
        for h in &self.hits {
            w.elem();
            h.write(&mut w);
        }
        w.end_arr();
        w.key("paths");
        paths_json(&mut w, &self.paths);
        w.end_obj();
        w.finish()
    }
}

/// A cause drill takes the bare id the tables print, or the full key.
pub fn drill_key_of(table: DrillTable, key: &str) -> String {
    if table == DrillTable::Cause && !key.starts_with("cause:") {
        format!("cause:{key}")
    } else {
        key.to_owned()
    }
}

/// The calls behind one row, first page.
pub fn drill(
    env: &StatsEnv<'_>,
    req: &ReportRequest,
    table: DrillTable,
    raw_key: &str,
) -> Result<DrillResult, StatsError> {
    let key = drill_key_of(table, raw_key);
    let _span = info_span!("Stats.drill", table = table.as_str()).entered();
    let target = DrillTarget {
        table,
        key: key.clone(),
    };
    let mut p = prepare(env, req, Some(&target))?;
    let corpus = std::mem::take(&mut p.detectors.corpus).finish();
    let redact = Redactor::new(true);
    let ctx = FinishCtx {
        corpus: &corpus,
        window: &p.window,
        redact: &redact,
        limit: p.limit,
        sample_cap: DEFAULT_SAMPLE_CAP,
        caveats: Vec::new(),
    };
    let d = &mut p.detectors;
    let hits = match table {
        DrillTable::Cause => d.causes.drill(&redact),
        DrillTable::Fail => d.fails.drill(&ctx),
        DrillTable::Time => d.time.drill(&ctx),
        DrillTable::Cluster => d.clusters.drill(&ctx),
        DrillTable::Wait => d.waiting.drill(&ctx),
    };
    if hits.is_empty() {
        return Err(StatsError::RowNotFound {
            table: table.as_str(),
            key,
        });
    }
    let total = hits.len();
    let shown: Vec<DrillHit> = hits.into_iter().take(DEFAULT_DRILL_PAGE).collect();
    let paths = paths_of(
        &p.result.paths,
        &redact,
        shown.iter().map(|h| h.sample.sid.as_str()),
    );
    Ok(DrillResult {
        table,
        key,
        total,
        offset: 0,
        hits: shown,
        paths,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshSummary {
    pub transcripts: usize,
    pub extracted: usize,
    pub cached: usize,
    pub skipped: Vec<String>,
    pub facts: usize,
}

/// Refresh the fact cache; `force` re-extracts (and clears it on a full scope).
pub fn refresh(
    env: &StatsEnv<'_>,
    agents: Option<&[Agent]>,
    force: bool,
) -> Result<RefreshSummary, StatsError> {
    let _span = info_span!("Stats.refresh").entered();
    let scope = scope_of(agents);
    if force && is_full_scope(&scope) {
        env.cache.clear();
    }
    let window = window_of(env.now_ms, ALL_DAYS)?;
    let (from_ms, to_ms) = window_ms(&window, env.local);
    let r = run_pass(
        &env.pass_env(),
        &PassArgs {
            scope: &scope,
            force,
            from_ms,
            to_ms,
            only: None,
        },
        &mut |_| {},
    );
    Ok(RefreshSummary {
        transcripts: r.transcripts,
        extracted: r.extracted,
        cached: r.cached,
        skipped: r.skipped,
        facts: r.facts,
    })
}
