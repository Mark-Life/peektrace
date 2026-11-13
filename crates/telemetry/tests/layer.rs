//! The store layer over a real subscriber: events collected in memory.

use std::sync::{Arc, Mutex, PoisonError};

use peektrace_telemetry::layer::{MAX_SPANS_PER_ROOT, mark_defect, panic_message};
use peektrace_telemetry::{Kind, Outcome, SpansOnly, StoreLayer, WideEvent};
use tracing::{field, info_span};
use tracing_subscriber::layer::{Layer as _, SubscriberExt as _};

fn traced(body: impl FnOnce()) -> Vec<WideEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let layer = StoreLayer::new(
        Arc::new(move |e| sink.lock().unwrap_or_else(PoisonError::into_inner).push(e)),
        "test",
        "p".to_owned(),
    );
    let subscriber = tracing_subscriber::registry().with(layer.with_filter(SpansOnly));
    tracing::subscriber::with_default(subscriber, body);
    let mut held = events.lock().unwrap_or_else(PoisonError::into_inner);
    std::mem::take(&mut *held)
}

fn root(kind: &'static str) -> tracing::Span {
    info_span!(
        "root",
        peektrace.root = true,
        peektrace.kind = kind,
        argv = "sessions ls",
        sessions.unreadable = field::Empty,
        error = field::Empty
    )
}

#[test]
fn one_success_event_with_its_child() {
    let events = traced(|| {
        let r = root("cli");
        let _in = r.enter();
        let _c = info_span!("child", id = "c1").entered();
    });
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(
        (e.name.as_str(), e.kind, &e.outcome),
        ("root", Kind::Cli, &Outcome::Success)
    );
    assert_eq!(e.attributes, r#"{"argv":"sessions ls"}"#);
    assert_eq!(e.spans.len(), 1);
    assert_eq!(e.spans[0].attributes, r#"{"id":"c1"}"#);
    assert_eq!(e.trace_id, "0000000000000001");
    assert!(
        e.to_json()
            .contains(r#""appVersion":"test","platform":"p""#)
    );
}

#[test]
fn failure_is_an_error_with_its_message() {
    let events = traced(|| {
        let r = root("cli");
        let _in = r.enter();
        info_span!("child").in_scope(|| std::thread::sleep(std::time::Duration::from_millis(1)));
        r.record("sessions.unreadable", 2);
        r.record("error", field::display("kaboom"));
    });
    let e = &events[0];
    assert_eq!(
        e.outcome,
        Outcome::Error {
            tag: "Error".to_owned(),
            message: "kaboom".to_owned()
        }
    );
    assert!(e.duration_ms > 0.0);
    assert_eq!(
        e.attributes,
        r#"{"argv":"sessions ls","sessions.unreadable":2}"#
    );
}

#[test]
fn panic_is_a_defect() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        mark_defect(&panic_message(info.payload()));
        previous(info);
    }));
    let events = traced(|| {
        let _ = std::panic::catch_unwind(|| {
            let _in = root("rpc").entered();
            panic!("kaboom");
        });
    });
    let e = &events[0];
    assert_eq!(e.kind, Kind::Rpc);
    assert!(matches!(&e.outcome, Outcome::Defect { message } if message == "kaboom"));
    assert!(
        e.to_json()
            .ends_with(r#""outcome":"defect","error":{"message":"kaboom"}}"#)
    );
}

#[test]
fn nested_roots_orphans_and_kinds() {
    let events = traced(|| {
        let _orphan = info_span!("orphan").entered();
        let outer = root("cli");
        let _in = outer.enter();
        let inner = info_span!(parent: None, "tui.sessions.list", peektrace.root = true, peektrace.kind = "tui");
        inner.in_scope(|| info_span!("Sessions.list").in_scope(|| {}));
        drop(inner);
        info_span!(parent: None, "lost").in_scope(|| {});
    });
    let names: Vec<_> = events
        .iter()
        .map(|e| (e.name.as_str(), e.kind, e.spans.len()))
        .collect();
    assert_eq!(
        names,
        [("tui.sessions.list", Kind::Rpc, 1), ("root", Kind::Cli, 0)]
    );
    assert_eq!(events[0].attributes, "{}");
}

#[test]
fn children_capped_per_root() {
    let events = traced(|| {
        let _in = root("cli").entered();
        for i in 0..=MAX_SPANS_PER_ROOT {
            info_span!("child", i).in_scope(|| {});
        }
    });
    let spans = &events[0].spans;
    assert_eq!(spans.len(), MAX_SPANS_PER_ROOT);
    assert_eq!(spans[MAX_SPANS_PER_ROOT - 1].attributes, r#"{"i":1023}"#);
}

#[test]
fn attribute_order_keeps_first_position() {
    let events = traced(|| {
        let r = info_span!(
            "cli",
            peektrace.root = true,
            b = 1,
            a = field::Empty,
            c = "x"
        );
        r.record("a", 2.5);
        r.record("b", 3);
        drop(r);
    });
    assert_eq!(events[0].attributes, r#"{"b":3,"c":"x","a":2.5}"#);
}

#[test]
fn finish_open_roots_emits_once_before_close() {
    let events = traced(|| {
        let r = root("cli");
        let _in = r.enter();
        drop(info_span!("child").entered());
        peektrace_telemetry::finish_open_roots();
        drop(info_span!("late").entered());
    });
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].outcome, Outcome::Success);
    assert_eq!(events[0].spans.len(), 1);
    assert_eq!(events[0].spans[0].name, "child");
}
