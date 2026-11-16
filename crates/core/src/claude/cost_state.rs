//! Claude Code's own cost record: `cost-state` lines. Each Claude Code
//! process rewrites a running total keyed by its `startTime`; the total
//! already counts subagents and internal calls.

use crate::js::value::JsValue;
use crate::sessions::model::NativeCost;

/// A process that starts this long after the first transcript line still
/// counts as covering the session start.
const START_SLACK_MS: f64 = 60_000.0;

/// The last record of one process.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Process {
    start_ms: f64,
    usd: f64,
    unknown_model_cost: bool,
}

/// `cost-state` lines folded to the last record per process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostStates {
    processes: Vec<Process>,
}

/// `totalCostUSD`, else the sum of `modelUsage[model].costUSD`.
fn usd_of(o: &JsValue) -> Option<f64> {
    let total = o.get("totalCostUSD").and_then(JsValue::as_num);
    total.or_else(|| {
        let usage = o.get("modelUsage")?.as_obj()?;
        Some(
            usage
                .entries()
                .filter_map(|(_, m)| m.get("costUSD").and_then(JsValue::as_num))
                .sum(),
        )
    })
}

/// Epoch ms of an ISO timestamp.
fn iso_ms(ts: &str) -> Option<f64> {
    #[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64")]
    ts.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond() as f64)
}

impl CostStates {
    /// Fold one `cost-state` line; a later line of the same process wins.
    pub fn push(&mut self, o: &JsValue) {
        let (Some(start_ms), Some(usd)) = (o.get("startTime").and_then(JsValue::as_num), usd_of(o))
        else {
            return;
        };
        let p = Process {
            start_ms,
            usd,
            unknown_model_cost: o.get("hasUnknownModelCost").is_some_and(JsValue::is_true),
        };
        match self
            .processes
            .iter_mut()
            .find(|q| q.start_ms.to_bits() == start_ms.to_bits())
        {
            Some(slot) => *slot = p,
            None => self.processes.push(p),
        }
    }

    /// The summed record, `None` at $0. It is partial when no process
    /// started by the first transcript line (a resumed process only), or
    /// when Claude Code flagged a model it could not price.
    pub fn native(&self, first_ts: Option<&str>) -> Option<NativeCost> {
        let usd: f64 = self.processes.iter().map(|p| p.usd).sum();
        if usd <= 0.0 {
            return None;
        }
        let earliest = self.processes.iter().map(|p| p.start_ms).reduce(f64::min)?;
        let covers_start = first_ts
            .and_then(iso_ms)
            .is_some_and(|first| earliest <= first + START_SLACK_MS);
        let unknown = self.processes.iter().any(|p| p.unknown_model_cost);
        Some(NativeCost {
            usd,
            includes_subagents: true,
            partial: !covers_start || unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start: f64, total: f64) -> JsValue {
        JsValue::parse(&format!(
            r#"{{"type":"cost-state","totalCostUSD":{total},"startTime":{start},"modelUsage":{{}}}}"#
        ))
        .expect("json")
    }

    const T0: &str = "2026-01-01T10:00:00.000Z";
    const T0_MS: f64 = 1_767_261_600_000.0;

    #[test]
    fn last_record_per_process_summed() {
        let mut c = CostStates::default();
        for (start, usd) in [(T0_MS - 5.0, 1.0), (T0_MS - 5.0, 2.0), (T0_MS + 9e6, 0.5)] {
            c.push(&line(start, usd));
        }
        let n = c.native(Some(T0)).expect("record");
        assert!((n.usd - 2.5).abs() < 1e-9);
        assert!(n.includes_subagents && !n.partial);
    }

    #[test]
    fn a_late_process_alone_is_partial() {
        let mut c = CostStates::default();
        c.push(&line(T0_MS + 3_600_000.0, 4.0));
        assert!(c.native(Some(T0)).is_some_and(|n| n.partial));
        assert!(c.native(None).is_some_and(|n| n.partial));
        assert_eq!(CostStates::default().native(Some(T0)), None);
        let mut zero = CostStates::default();
        zero.push(&line(T0_MS, 0.0));
        assert_eq!(zero.native(Some(T0)), None, "$0 is no record");
    }

    #[test]
    fn model_usage_backs_up_a_missing_total() {
        let mut c = CostStates::default();
        let o = JsValue::parse(&format!(
            r#"{{"startTime":{T0_MS},"hasUnknownModelCost":true,"modelUsage":{{"a":{{"costUSD":1.5}},"b":{{"costUSD":0.25}}}}}}"#
        ))
        .expect("json");
        c.push(&o);
        let n = c.native(Some(T0)).expect("record");
        assert!((n.usd - 1.75).abs() < 1e-9);
        assert!(n.partial, "an unpriced model makes the record partial");
    }
}
