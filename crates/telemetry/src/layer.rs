//! A `tracing` layer that builds one wide event per root span: a span with
//! `peektrace.root = true`. Every other span joins its nearest root
//! ancestor; one with none is dropped. Events go to a callback when their
//! root closes, so the layer never touches the store itself.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tracing::span::{Attributes, Id, Record};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Filter, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::attrs::{Attrs, ROOT_FIELD, object};
use crate::event::{Kind, Outcome, SpanRecord, WideEvent, hex_id, uuid_v4};

/// Child spans kept per root; later ones are dropped.
pub const MAX_SPANS_PER_ROOT: usize = 1024;
/// TS has no tag at the Rust record sites.
const ERROR_TAG: &str = "Error";

/// Where finished events go.
pub type Emit = Arc<dyn Fn(WideEvent) + Send + Sync>;

/// The finished children of one open root.
#[derive(Default)]
struct RootBuf {
    spans: Mutex<Vec<SpanRecord>>,
    full: AtomicBool,
    closed: AtomicBool,
}

impl RootBuf {
    fn push(&self, record: SpanRecord) {
        if self.closed.load(Ordering::Relaxed) {
            return;
        }
        let mut spans = self.spans.lock().unwrap_or_else(PoisonError::into_inner);
        if spans.len() < MAX_SPANS_PER_ROOT {
            spans.push(record);
        }
        if spans.len() >= MAX_SPANS_PER_ROOT {
            self.full.store(true, Ordering::Relaxed);
        }
    }

    fn take(&self) -> Vec<SpanRecord> {
        self.closed.store(true, Ordering::Relaxed);
        std::mem::take(&mut *self.spans.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// What the layer keeps on each span it tracks.
struct Node {
    start: Instant,
    /// `None` once the root is full: the span would be dropped anyway.
    attrs: Option<Attrs>,
    root: Arc<RootBuf>,
    /// The trace id, on roots only.
    trace: Option<u64>,
}

thread_local! {
    /// Roots entered on this thread, innermost last.
    static ENTERED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Roots a panic hit while they were entered, with the panic message.
static DEFECTS: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());

/// Mark the innermost root entered on this thread as a defect. A root
/// already marked keeps its first message.
pub fn mark_defect(message: &str) {
    if let Some(id) = ENTERED.with(|e| e.borrow().last().copied()) {
        let mut defects = DEFECTS.lock().unwrap_or_else(PoisonError::into_inner);
        if defects.iter().all(|(d, _)| *d != id) {
            defects.push((id, message.to_owned()));
        }
    }
}

fn take_defect(id: u64) -> Option<String> {
    let mut defects = DEFECTS.lock().unwrap_or_else(PoisonError::into_inner);
    let at = defects.iter().position(|(d, _)| *d == id)?;
    Some(defects.swap_remove(at).1)
}

/// The text of a panic payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned())
}

/// Milliseconds since `start`, from whole nanoseconds as TS divides them.
#[expect(
    clippy::cast_precision_loss,
    reason = "span durations are far below 2^53 ns"
)]
fn ms_since(start: Instant) -> f64 {
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}

/// Builds wide events and hands each to `emit`.
pub struct StoreLayer {
    emit: Emit,
    app_version: String,
    platform: String,
    /// Shared by span and trace ids, as the TS tracer counts them.
    counter: AtomicU64,
    /// Ids of roots not yet emitted.
    open: Mutex<Vec<u64>>,
}

impl StoreLayer {
    pub fn new(emit: Emit, app_version: &str, platform: String) -> Self {
        Self {
            emit,
            app_version: app_version.to_owned(),
            platform,
            counter: AtomicU64::new(0),
            open: Mutex::new(Vec::new()),
        }
    }

    fn open_roots(&self) -> std::sync::MutexGuard<'_, Vec<u64>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Emit every root still open, as if it closed now. For an exit that
    /// never unwinds back to the code holding the root.
    pub fn finish_open<S: for<'a> LookupSpan<'a>>(&self, spans: &S) {
        let open = std::mem::take(&mut *self.open_roots());
        for raw in open {
            let id = Id::from_u64(raw);
            let Some(span) = spans.span(&id) else {
                continue;
            };
            let node = span.extensions_mut().remove::<Node>();
            if let Some(mut node) = node {
                self.emit_root(&id, span.name(), &mut node);
            }
        }
    }

    fn emit_root(&self, id: &Id, name: &str, node: &mut Node) {
        if let Some(attrs) = node.attrs.take() {
            let defect = take_defect(id.into_u64());
            (self.emit)(self.event(name, node, &attrs, defect));
        }
    }

    fn next_id(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn event(&self, name: &str, node: &Node, attrs: &Attrs, defect: Option<String>) -> WideEvent {
        let duration_ms = ms_since(node.start);
        let outcome = match (defect, attrs.error()) {
            (Some(message), _) => Outcome::Defect { message },
            (None, Some(message)) => Outcome::Error {
                tag: ERROR_TAG.to_owned(),
                message: message.to_owned(),
            },
            (None, None) => Outcome::Success,
        };
        let attributes = object(
            attrs
                .fields()
                .iter()
                .filter(|(k, _)| !k.starts_with("peektrace."))
                .map(|(k, v)| (*k, v.as_str())),
        );
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        WideEvent {
            id: uuid_v4(),
            trace_id: hex_id(node.trace.unwrap_or_default()),
            ts,
            kind: Kind::of(attrs.kind()),
            name: name.to_owned(),
            app_version: self.app_version.clone(),
            platform: self.platform.clone(),
            duration_ms,
            attributes,
            spans: node.root.take(),
            outcome,
        }
    }
}

impl<S> Layer<S> for StoreLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let declares_root = attrs.metadata().fields().field(ROOT_FIELD).is_some();
        let node = match declares_root.then(|| recorded(attrs)) {
            Some(fields) if fields.is_root() => {
                let trace = self.next_id();
                self.next_id();
                self.open_roots().push(id.into_u64());
                Node {
                    start: Instant::now(),
                    attrs: Some(fields),
                    root: Arc::new(RootBuf::default()),
                    trace: Some(trace),
                }
            }
            fields => {
                self.next_id();
                let Some(root) = span
                    .parent()
                    .and_then(|p| p.extensions().get::<Node>().map(|n| Arc::clone(&n.root)))
                else {
                    return;
                };
                let full = root.full.load(Ordering::Relaxed);
                Node {
                    start: Instant::now(),
                    attrs: (!full).then(|| fields.unwrap_or_else(|| recorded(attrs))),
                    root,
                    trace: None,
                }
            }
        };
        span.extensions_mut().insert(node);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(node) = span.extensions_mut().get_mut::<Node>()
            && let Some(attrs) = node.attrs.as_mut()
        {
            values.record(attrs);
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if is_root(id, &ctx) {
            ENTERED.with(|e| e.borrow_mut().push(id.into_u64()));
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if is_root(id, &ctx) {
            let raw = id.into_u64();
            ENTERED.with(|e| {
                let mut e = e.borrow_mut();
                if let Some(at) = e.iter().rposition(|x| *x == raw) {
                    e.remove(at);
                }
            });
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(mut node) = span.extensions_mut().remove::<Node>() else {
            return;
        };
        if node.trace.is_some() {
            let raw = id.into_u64();
            self.open_roots().retain(|x| *x != raw);
            self.emit_root(&id, span.name(), &mut node);
            return;
        }
        let Some(attrs) = node.attrs.take() else {
            return;
        };
        let duration_ms = ms_since(node.start);
        node.root.push(SpanRecord {
            name: span.name().to_owned(),
            duration_ms,
            attributes: attrs.to_json(),
        });
    }
}

fn is_root<S>(id: &Id, ctx: &Context<'_, S>) -> bool
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    ctx.span(id)
        .and_then(|s| s.extensions().get::<Node>().map(|n| n.trace.is_some()))
        .unwrap_or(false)
}

fn recorded(attrs: &Attributes<'_>) -> Attrs {
    let mut fields = Attrs::default();
    attrs.record(&mut fields);
    fields
}

/// Lets spans through and keeps log events out, decided once per callsite.
pub struct SpansOnly;

impl<S> Filter<S> for SpansOnly {
    fn enabled(&self, meta: &Metadata<'_>, _: &Context<'_, S>) -> bool {
        meta.is_span()
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        if meta.is_span() {
            tracing::subscriber::Interest::always()
        } else {
            tracing::subscriber::Interest::never()
        }
    }
}
