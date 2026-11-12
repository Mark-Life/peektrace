//! Tracing setup. `--otel` (or a non-empty `PEEKTRACE_OTEL`) echoes each
//! finished span to stderr in the TS format:
//! `[otel] <name> <ms>ms ok|fail {attrs}`. A span fails when it records
//! `error`. `--log-level` prints log events.
//! The telemetry store layer records wide events. With none of the three,
//! no subscriber is installed and spans cost nothing.

use std::io::Write as _;
use std::time::Instant;

use peektrace_core::js::num;
use peektrace_telemetry::{Attrs, SpansOnly, StoreLayer};

use crate::argv;
use tracing::span::{Attributes, Id, Record};
use tracing::{Level, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Lines kept back while the TUI owns the terminal; `None` when stderr is
/// written directly.
static HELD: std::sync::Mutex<Option<std::collections::VecDeque<Vec<u8>>>> =
    std::sync::Mutex::new(None);
/// Lines kept while held; older ones drop.
const HOLD_MAX: usize = 10_000;

fn emit(bytes: &[u8]) {
    let mut held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match held.as_mut() {
        Some(q) => {
            if q.len() >= HOLD_MAX {
                q.pop_front();
            }
            q.push_back(bytes.to_vec());
        }
        None => {
            let _ = std::io::stderr().lock().write_all(bytes);
        }
    }
}

/// Keep span and log lines back until [`release`].
pub fn hold() {
    *HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(std::collections::VecDeque::new());
}

/// Print the lines kept back, then write directly again.
pub fn release() {
    let held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let mut err = std::io::stderr().lock();
    for line in held.into_iter().flatten() {
        let _ = err.write_all(&line);
    }
}

/// stderr, or the hold buffer while the TUI runs.
struct HoldWriter;

impl std::io::Write for HoldWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        emit(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Timing {
    start: Instant,
    attrs: Attrs,
}

/// Prints each span as it closes, children before parents.
struct EchoLayer;

impl<S> Layer<S> for EchoLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Attrs::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Timing {
                start: Instant::now(),
                attrs: fields,
            });
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(t) = span.extensions_mut().get_mut::<Timing>()
        {
            values.record(&mut t.attrs);
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let ext = span.extensions();
        let Some(t) = ext.get::<Timing>() else {
            return;
        };
        let ms = t.start.elapsed().as_secs_f64() * 1000.0;
        let status = if t.attrs.failed() { "fail" } else { "ok" };
        let attrs = if t.attrs.is_empty() {
            String::new()
        } else {
            format!(" {}", t.attrs.to_json())
        };
        let line = format!(
            "[otel] {} {}ms {status}{attrs}\n",
            span.name(),
            num::to_fixed(ms, 1)
        );
        emit(line.as_bytes());
    }
}

/// `--log-level` values, as `@effect/cli` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogLevel {
    All,
    Trace,
    Debug,
    Info,
    Warning,
    Error,
    Fatal,
    None,
}

impl LogLevel {
    fn filter(self) -> LevelFilter {
        match self {
            Self::All | Self::Trace => LevelFilter::TRACE,
            Self::Debug => LevelFilter::DEBUG,
            Self::Info => LevelFilter::INFO,
            Self::Warning => LevelFilter::WARN,
            Self::Error | Self::Fatal => LevelFilter::ERROR,
            Self::None => LevelFilter::OFF,
        }
    }
}

/// True when warning lines print at `level`; absent means the default level,
/// which shows them.
pub fn shows_warnings(level: Option<LogLevel>) -> bool {
    level.is_none_or(|l| l.filter() >= LevelFilter::WARN)
}

/// Install a subscriber only when spans, logs or the telemetry store are
/// wanted.
pub fn init(echo: bool, log_level: Option<LogLevel>, store: Option<StoreLayer>) {
    if !echo && log_level.is_none() && store.is_none() {
        return;
    }
    let logs = log_level.map(|l| {
        tracing_subscriber::fmt::layer()
            .with_writer(|| HoldWriter)
            .with_target(false)
            .with_filter(l.filter())
    });
    let echo = echo.then_some(EchoLayer.with_filter(LevelFilter::from_level(Level::TRACE)));
    let store = store.map(|s| s.with_filter(SpansOnly));
    let _ = tracing_subscriber::registry()
        .with(store)
        .with(echo)
        .with(logs)
        .try_init();
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
        .floor()
}

/// Start the subscriber: the `--otel` echo, log lines and the telemetry
/// store, each decided from the raw command line as TS does.
pub fn start(raw: &[String], version: &str) {
    let flags = argv::before_separator(raw);
    let echo = flags.iter().any(|a| a == "--otel")
        || std::env::var("PEEKTRACE_OTEL").is_ok_and(|v| !v.is_empty());
    let on = argv::telemetry_on(raw, std::env::var_os("PEEKTRACE_NO_TELEMETRY").is_some());
    let store = on.then(|| {
        let home = std::env::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = std::env::var_os("PEEKTRACE_DIR").map(|d| d.to_string_lossy().into_owned());
        let dir = peektrace_telemetry::store::dir_of(dir.as_deref(), &home).ok();
        #[expect(clippy::cast_possible_truncation, reason = "epoch ms fit i64")]
        let now = now_ms() as i64;
        peektrace_telemetry::start(dir, version, now)
    });
    init(echo, argv::log_level(raw), store);
}
