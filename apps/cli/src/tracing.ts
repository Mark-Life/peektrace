/** Optional, zero-overhead-when-off tracing for the CLI.
 *
 * Every core IO op is already wrapped in `Effect.withSpan`; those spans go to a
 * no-op tracer by default (no exporter, no allocation beyond Effect's own). When
 * `--otel` is passed or `PEEKTRACE_OTEL` is set, we install a tiny console span
 * exporter so each span prints `name durationMs ok/fail {attrs}` to stderr. This
 * keeps the dependency footprint at zero (no `@effect/opentelemetry`) while still
 * making the existing instrumentation observable on demand.
 *
 * To export to a real OTLP collector instead, swap `consoleTracer` for the
 * `@effect/opentelemetry` `NodeSdk` layer behind the same flag — the spans on the
 * core services need no change.
 */
import { Layer, Option, Tracer } from "effect";

const NS_PER_MS = 1_000_000;
const HEX_RADIX = 16;
const SPAN_ID_WIDTH = 8;

let spanCounter = 0;
const nextId = (): string => {
  spanCounter += 1;
  return spanCounter.toString(HEX_RADIX).padStart(SPAN_ID_WIDTH, "0");
};

/** A minimal `Tracer` that logs each finished span to stderr. */
const consoleTracer: Tracer.Tracer = Tracer.make({
  span(name, parent, context, links, startTime, kind, options) {
    const attributes = new Map<string, unknown>(
      Object.entries(options?.attributes ?? {})
    );
    const traceId = Option.match(parent, {
      onNone: () => nextId(),
      onSome: (p) => p.traceId,
    });
    const span: Tracer.Span = {
      _tag: "Span",
      name,
      spanId: nextId(),
      traceId,
      parent,
      context,
      status: { _tag: "Started", startTime },
      attributes,
      links,
      sampled: true,
      kind,
      end(endTime, exit) {
        const durationMs = (Number(endTime - startTime) / NS_PER_MS).toFixed(1);
        const outcome = exit._tag === "Success" ? "ok" : "fail";
        const attrs =
          attributes.size > 0
            ? ` ${JSON.stringify(Object.fromEntries(attributes))}`
            : "";
        process.stderr.write(
          `[otel] ${name} ${durationMs}ms ${outcome}${attrs}\n`
        );
      },
      attribute(key, value) {
        attributes.set(key, value);
      },
      event() {
        /* events are not exported by the console tracer */
      },
      addLinks() {
        /* links are not exported by the console tracer */
      },
    };
    return span;
  },
  context(f) {
    return f();
  },
});

/** True when console tracing is requested via flag or `PEEKTRACE_OTEL` env. */
export const otelEnabled = (flag = false): boolean =>
  flag ||
  (process.env.PEEKTRACE_OTEL !== undefined &&
    process.env.PEEKTRACE_OTEL !== "");

/**
 * The tracing layer: a console span tracer when `enabled`, else `Layer.empty`
 * (the default no-op tracer — no exporter, no startup cost).
 */
export const tracingLayer = (enabled: boolean): Layer.Layer<never> =>
  enabled ? Layer.setTracer(consoleTracer) : Layer.empty;
