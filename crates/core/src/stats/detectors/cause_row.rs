//! One row of the cause table and its JSON shape.

use super::causes::CauseKind;
use super::rules::{RuleCheck, Split};
use crate::js::json_out::Writer;
use crate::stats::accumulate::{SampleRef, write_samples};
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
