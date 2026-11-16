//! Which turn each event belongs to, and where the dumb zone was crossed.

use std::collections::HashMap;

use peektrace_protocol::view::AnalyzedSession;

/// The 1-based turn of every event: the last turn whose request id an
/// event named, 0 before the first. A request id shared by two turns names
/// the later one.
pub fn turn_numbers(a: &AnalyzedSession) -> Vec<usize> {
    let by_request: HashMap<&str, usize> = a
        .turns
        .iter()
        .enumerate()
        .map(|(i, t)| (t.request_id.as_str(), i + 1))
        .collect();
    let mut cur = 0;
    a.events
        .iter()
        .map(|e| {
            if let Some(&n) = e
                .request_id
                .as_deref()
                .filter(|r| !r.is_empty())
                .and_then(|r| by_request.get(r))
            {
                cur = n;
            }
            cur
        })
        .collect()
}

/// The first event of the turn that crossed into the dumb zone. A crossing
/// turn missing from `turns` matches the first event with no request id,
/// as TS compares `undefined` with `undefined`.
pub fn cross_event_index(a: &AnalyzedSession) -> Option<usize> {
    let cross = usize::try_from(a.dumb_zone_cross_turn).ok()?;
    let request = a.turns.get(cross).map(|t| t.request_id.as_str());
    a.events
        .iter()
        .position(|e| e.request_id.as_deref() == request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixtures::{analysis, event, turn};

    #[test]
    fn numbers_follow_request_ids() {
        let mut a = analysis();
        a.turns = vec![turn("r1"), turn("r2")];
        a.events = ["", "r1", "", "r2", "rX", "r1"]
            .into_iter()
            .map(event)
            .collect();
        assert_eq!(turn_numbers(&a), [0, 1, 1, 2, 2, 1]);
        a.dumb_zone_cross_turn = 1;
        assert_eq!(cross_event_index(&a), Some(3));
        a.dumb_zone_cross_turn = -1;
        assert_eq!(cross_event_index(&a), None);
        a.dumb_zone_cross_turn = 7;
        assert_eq!(cross_event_index(&a), Some(0));
    }
}
