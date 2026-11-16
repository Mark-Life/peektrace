//! Tracing setup. `--otel` (or a non-empty `PEEKTRACE_OTEL`) echoes each
//! finished span to stderr in the TS format:
//! `[otel] <name> <ms>ms ok|fail {attrs}`. A span fails when it records
//! `error`. `--log-level` prints log events.
//! With neither, no subscriber is installed and spans cost nothing.

use std::fmt;
use std::io::Write as _;
use std::time::Instant;

use peektrace_core::js::num;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Level, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;

use peektrace_core::js::json_out::push_str;

/// Field that marks a span failed. It is not printed: TS takes `fail` from
/// the span's exit and prints only its attributes.
const FAIL_FIELD: &str = "error";

/// Span attributes, kept as the JSON they print as, plus the failure mark.
#[derive(Default)]
struct Attrs {
    fields: Vec<(&'static str, String)>,
    failed: bool,
}

impl Attrs {
    fn set(&mut self, key: &'static str, json: String) {
        if key == FAIL_FIELD {
            self.failed = true;
            return;
        }
        match self.fields.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = json,
            None => self.fields.push((key, json)),
        }
    }

    fn quoted(s: &str) -> String {
        let mut out = String::new();
        push_str(&mut out, s);
        out
    }

    fn to_json(&self) -> String {
        let body = self
            .fields
            .iter()
            .map(|(k, v)| format!("{}:{v}", Self::quoted(k)))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{body}}}")
    }
}

impl Visit for Attrs {
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.set(field.name(), value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.set(field.name(), value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.set(field.name(), value.to_string());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.set(field.name(), num::to_json(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field.name(), Self::quoted(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.set(field.name(), Self::quoted(&format!("{value:?}")));
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
        let status = if t.attrs.failed { "fail" } else { "ok" };
        let attrs = if t.attrs.fields.is_empty() {
            String::new()
        } else {
            format!(" {}", t.attrs.to_json())
        };
        let line = format!(
            "[otel] {} {}ms {status}{attrs}\n",
            span.name(),
            num::to_fixed(ms, 1)
        );
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
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

/// Install a subscriber only when spans or logs are wanted.
pub fn init(echo: bool, log_level: Option<LogLevel>) {
    if !echo && log_level.is_none() {
        return;
    }
    let logs = log_level.map(|l| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .with_filter(l.filter())
    });
    let echo = echo.then_some(EchoLayer.with_filter(LevelFilter::from_level(Level::TRACE)));
    let _ = tracing_subscriber::registry()
        .with(echo)
        .with(logs)
        .try_init();
}
