//! The paged routes record whether the analysis came from the cache.

mod common;

use std::sync::{Arc, Mutex, PoisonError};

use common::{Options, SESSION, start};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;

type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

/// Every `sessions.cache` and `history.*` value, with its span name.
struct Capture(Seen);

struct Fields<'a>(&'a mut Vec<(String, String)>);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }
}

impl Capture {
    fn keep(&self, span: &str, values: Vec<(String, String)>) {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.extend(
            values
                .into_iter()
                .filter(|(k, _)| k == "sessions.cache" || k.starts_with("history."))
                .map(|(k, v)| (span.to_owned(), k, v)),
        );
    }
}

impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(&self, attrs: &Attributes<'_>, _: &Id, _: Context<'_, S>) {
        let mut v = Vec::new();
        attrs.record(&mut Fields(&mut v));
        self.keep(attrs.metadata().name(), v);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut v = Vec::new();
        values.record(&mut Fields(&mut v));
        let name = ctx.span(id).map(|s| s.name()).unwrap_or_default();
        self.keep(name, v);
    }
}

#[test]
fn a_second_call_is_a_cache_hit_and_the_query_is_not_recorded() {
    let seen: Seen = Arc::default();
    let subscriber = tracing_subscriber::registry().with(Capture(Arc::clone(&seen)));
    tracing::subscriber::set_global_default(subscriber).expect("subscriber");
    let s = start(&Options {
        name: "history-spans",
        ..Options::default()
    });
    assert_eq!(
        s.get(&format!("/api/sessions/{SESSION}/summary")).status,
        200
    );
    assert_eq!(
        s.get(&format!("/api/sessions/{SESSION}/growth")).status,
        200
    );
    let page = format!("/api/sessions/{SESSION}/history?offset=0&limit=7");
    assert_eq!(s.get(&page).status, 200);
    let search = format!("/api/sessions/{SESSION}/history/search?q=secretword");
    assert_eq!(s.get(&search).status, 200);
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let has = |span: &str, k: &str, v: &str| {
        seen.iter()
            .any(|(s, kk, vv)| s == span && kk == k && vv == v)
    };
    assert!(
        has("rpc.sessions.summary", "sessions.cache", "miss"),
        "{seen:?}"
    );
    assert!(
        has("rpc.sessions.growth", "sessions.cache", "hit"),
        "{seen:?}"
    );
    assert!(
        has("rpc.sessions.history", "history.limit", "7"),
        "{seen:?}"
    );
    assert!(
        has("rpc.sessions.history", "history.offset", "0"),
        "{seen:?}"
    );
    assert!(
        has("rpc.sessions.historySearch", "history.query_len", "10"),
        "{seen:?}"
    );
    assert!(
        has("rpc.sessions.historySearch", "history.matches", "0"),
        "{seen:?}"
    );
    assert!(!seen.iter().any(|(_, _, v)| v.contains("secretword")));
}
