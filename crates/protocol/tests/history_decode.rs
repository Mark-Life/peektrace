//! The paged session parts decode with their hashes, as the inspector's
//! `api.test.ts` reads them.

use peektrace_protocol::SummaryHistory;
use peektrace_protocol::view::HistoryPage;
use serde_json::Value;

#[test]
fn a_summary_and_a_page_decode_with_their_hashes() {
    let summary: Value = serde_json::from_str(
        r#"{"provider":"claude","sessionId":"s","history":{"rev":"r","total":101,"pageSize":100,"pages":["a","b"],"index":"i","growth":"g"}}"#,
    )
    .expect("summary json");
    let history: SummaryHistory =
        serde_json::from_value(summary["history"].clone()).expect("history");
    assert_eq!(history.pages, ["a", "b"]);
    assert_eq!(history.total, 101);
    let page: HistoryPage = serde_json::from_str(
        r#"{"rev":"r","offset":100,"total":101,"hash":"b","events":[{"index":7,"kind":"assistant-text","title":"","preview":"","body":"x","tokensEst":1}]}"#,
    )
    .expect("page");
    assert_eq!(Some(&page.meta.hash), history.pages.get(1));
    assert_eq!(page.meta.offset, 100);
    let bodies: Vec<&str> = page.events.iter().map(|e| e.body.as_str()).collect();
    assert_eq!(bodies, ["x"]);
    let back: HistoryPage =
        serde_json::from_value(serde_json::to_value(&page).expect("value")).expect("again");
    assert_eq!(back, page);
}
