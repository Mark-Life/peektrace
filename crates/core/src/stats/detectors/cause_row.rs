//! One row of the cause table and its JSON shape.

use super::causes::CauseKind;
use super::rules::{RuleCheck, Split};
use crate::js::json_out::Writer;
use crate::stats::accumulate::{CorpusSummary, SampleRef, write_samples};
use crate::stats::schema::{CauseId, WriteExt};

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
    /// Hits minus misses already fixed (installed binaries); command not found only.
    pub live_hits: Option<f64>,
    pub flagged: f64,
    pub hits_by: Split,
    pub pairs: f64,
    pub pairs_by: Split,
    pub retry_sessions: f64,
    pub sessions: f64,
    pub projects: f64,
    pub retry_sec: f64,
    /// Every pair's failed call was a shell command, so its time is a share
    /// of shell time; otherwise of all tool time.
    pub shell: bool,
    /// `retry_sec` over shell time, or over tool time when not `shell`.
    pub retry_share: f64,
    /// Re-read tokens priced at each session's own model; approximate.
    pub retry_usd: Option<f64>,
    /// Re-read tokens by transcript, for [`Self::retry_usd`]; not written.
    pub ctx_by_sid: Vec<(String, f64)>,
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
        if let Some(n) = self.live_hits {
            w.key("liveHits").num(n);
        }
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
        w.key("shell").bool(self.shell);
        w.key("retryShare").num(self.retry_share);
        match self.retry_usd {
            Some(usd) => w.key("retryUsd").num(usd),
            None => w.key("retryUsd").null(),
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

/// Seconds over the time they are a share of: shell time for shell causes,
/// all tool time otherwise.
pub fn retry_share(sec: f64, shell: bool, corpus: &CorpusSummary) -> f64 {
    let of = if shell {
        corpus.bash_sec
    } else {
        corpus.tool_sec
    };
    if of > 0.0 { sec / of } else { 0.0 }
}

/// Every cause added up: what the verdict and the totals row state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CauseTotals {
    pub hits: f64,
    pub pairs: f64,
    pub retry_sec: f64,
    /// True when every cause with a pair is a shell cause.
    pub shell: bool,
    pub retry_share: f64,
    pub retry_ctx_tokens: f64,
    /// `None` when no cause could be priced.
    pub retry_usd: Option<f64>,
    /// Pairs that are read-backs of large output, not failures.
    pub non_failing_pairs: f64,
    /// Pairs with no named cause, so no fix.
    pub unclassified_pairs: f64,
}

impl CauseTotals {
    pub fn of(rows: &[CauseRow], corpus: &CorpusSummary) -> Self {
        let sum = |f: fn(&CauseRow) -> f64| rows.iter().map(f).sum::<f64>();
        let shell = rows.iter().filter(|r| r.pairs > 0.0).all(|r| r.shell);
        let retry_sec = sum(|r| r.retry_sec);
        let priced: Vec<f64> = rows.iter().filter_map(|r| r.retry_usd).collect();
        Self {
            hits: sum(|r| r.hits),
            pairs: sum(|r| r.pairs),
            retry_sec,
            shell,
            retry_share: retry_share(retry_sec, shell, corpus),
            retry_ctx_tokens: sum(|r| r.retry_ctx_tokens),
            retry_usd: (!priced.is_empty()).then(|| priced.iter().sum()),
            non_failing_pairs: sum(|r| if r.failing { 0.0 } else { r.pairs }),
            unclassified_pairs: sum(|r| {
                if r.cause == CauseId::Unclassified {
                    r.pairs
                } else {
                    0.0
                }
            }),
        }
    }

    pub fn write(&self, w: &mut Writer) {
        w.begin_obj();
        w.key("hits").num(self.hits);
        w.key("pairs").num(self.pairs);
        w.key("retrySec").num(self.retry_sec);
        w.key("shell").bool(self.shell);
        w.key("retryShare").num(self.retry_share);
        w.key("retryCtxTokens").num(self.retry_ctx_tokens);
        match self.retry_usd {
            Some(usd) => w.key("retryUsd").num(usd),
            None => w.key("retryUsd").null(),
        }
        w.key("nonFailingPairs").num(self.non_failing_pairs);
        w.key("unclassifiedPairs").num(self.unclassified_pairs);
        w.end_obj();
    }
}
