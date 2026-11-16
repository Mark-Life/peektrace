//! The report and drill results and their JSON shapes.

use std::collections::HashMap;

use super::accumulate::{CorpusSummary, DrillHit, DrillTable, SampleRef, StatsWindow};
use super::detectors::clustering::ClusterRow;
use super::detectors::corpus::CorpusPanels;
use super::detectors::failures::FailRow;
use super::detectors::retries::CauseRow;
use super::detectors::timing::TimeRow;
use super::detectors::waiting::WaitPanel;
use super::markers::SessionMarker;
use super::rank::uniq_sorted;
use super::redact::Redactor;
use super::schema::{DETECTOR_VERSION, STATS_SCHEMA_VERSION};
use crate::js::json_out::Writer;

/// The versioned report.
#[derive(Debug, Clone, Default)]
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

pub(super) fn paths_of<'s>(
    paths: &HashMap<String, String>,
    redact: &Redactor,
    sids: impl IntoIterator<Item = &'s str>,
) -> Vec<(String, String)> {
    uniq_sorted(sids)
        .into_iter()
        .filter_map(|sid| paths.get(&sid).map(|p| (sid.clone(), redact.text(p))))
        .collect()
}

pub(super) fn samples_in(r: &StatsReport) -> Vec<&SampleRef> {
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

#[derive(Debug, Clone)]
pub struct DrillResult {
    pub table: DrillTable,
    pub key: String,
    pub total: usize,
    pub offset: f64,
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
        w.key("total")
            .int(i64::try_from(self.total).unwrap_or(i64::MAX));
        w.key("offset").num(self.offset);
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

/// `SessionMarkers`: the marks one session's calls get.
pub fn markers_json(sid: &str, markers: &[SessionMarker], pretty: bool) -> String {
    let mut w = Writer::new(pretty);
    w.begin_obj();
    w.key("schemaVersion").str(STATS_SCHEMA_VERSION);
    w.key("sid").text(sid);
    w.key("detectorVersion").num(DETECTOR_VERSION);
    w.key("markers").begin_arr();
    for m in markers {
        w.elem();
        m.write(&mut w);
    }
    w.end_arr();
    w.end_obj();
    w.finish()
}
