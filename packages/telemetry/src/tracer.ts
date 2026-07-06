import { Cause, Effect, Exit, Layer, Option, Runtime, Tracer } from "effect";
import { writeConsoleSpan } from "./console";
import type { WideEvent } from "./schema";
import { TelemetryStore } from "./store";

const NS_PER_MS = 1_000_000;
const HEX_RADIX = 16;
const ID_WIDTH = 16;
// Bounds so a long-lived root (e.g. the `cli` root under `serve`, which only
// ends at shutdown) cannot accumulate child spans without limit. `serve` runs
// `Effect.never`, and the platform wraps every HTTP request in an `http.server`
// span that buffers under the never-ending cli traceId; these caps keep total
// buffered memory bounded to MAX_BUFFERS * MAX_SPANS_PER_ROOT records.
const MAX_BUFFERS = 1024;
const MAX_SPANS_PER_ROOT = 1024;
const PLATFORM = `${process.platform}; bun/${process.versions.bun}; node/${process.version}`;

const outcomeOf = (exit: Exit.Exit<unknown, unknown>) => {
  if (Exit.isSuccess(exit)) {
    return { outcome: "success" as const };
  }
  const fail = Cause.failureOption(exit.cause);
  if (Option.isSome(fail)) {
    const e = (fail.value ?? {}) as {
      _tag?: unknown;
      message?: unknown;
    } & Record<string, unknown>;
    const { _tag, message, ...fields } = e;
    return {
      outcome: "error" as const,
      error: {
        tag: typeof _tag === "string" ? _tag : "Error",
        message: typeof message === "string" ? message : String(e),
        fields,
      },
    };
  }
  return {
    outcome: "defect" as const,
    error: { message: Cause.pretty(exit.cause) },
  };
};

/**
 * A tracer that assembles one wide event per `peephole.root` span and flushes
 * it synchronously to the {@link TelemetryStore} on root-span end.
 */
export const TelemetryTracerLive = (opts: {
  echo: boolean;
  appVersion: string;
}) =>
  Layer.unwrapEffect(
    Effect.gen(function* () {
      const store = yield* TelemetryStore;
      const runtime = yield* Effect.runtime<never>();
      const flush = Runtime.runSyncExit(runtime);

      const buffers = new Map<string, WideEvent["spans"][number][]>();
      let counter = 0;
      const nextId = () => {
        counter += 1;
        return counter.toString(HEX_RADIX).padStart(ID_WIDTH, "0");
      };

      /** Buffer a finished child span under its root's traceId, bounded. */
      const bufferChild = (
        traceId: string,
        record: WideEvent["spans"][number]
      ) => {
        const existing = buffers.get(traceId);
        if (existing) {
          if (existing.length < MAX_SPANS_PER_ROOT) {
            existing.push(record);
          }
          return;
        }
        if (buffers.size >= MAX_BUFFERS) {
          const oldest = buffers.keys().next().value;
          if (oldest !== undefined) {
            buffers.delete(oldest);
          }
        }
        buffers.set(traceId, [record]);
      };

      /** Assemble the wide event for a finished root span and flush it. */
      const flushRoot = (root: {
        name: string;
        traceId: string;
        durationMs: number;
        attributes: ReadonlyMap<string, unknown>;
        exit: Exit.Exit<unknown, unknown>;
      }) => {
        const spans = buffers.get(root.traceId) ?? [];
        buffers.delete(root.traceId);
        const attrs = Object.fromEntries(
          [...root.attributes].filter(([k]) => !k.startsWith("peephole."))
        );
        const event = {
          id: crypto.randomUUID(),
          traceId: root.traceId,
          ts: Date.now(),
          kind:
            (root.attributes.get("peephole.kind") as "cli" | "rpc") ?? "cli",
          name: root.name,
          appVersion: opts.appVersion,
          platform: PLATFORM,
          durationMs: root.durationMs,
          attributes: attrs,
          spans,
          ...outcomeOf(root.exit),
        } as WideEvent;
        flush(store.record(event));
      };

      const tracer = Tracer.make({
        span(name, parent, context, links, startTime, kind, options) {
          const isRoot = options?.attributes?.["peephole.root"] === true;
          const traceId = isRoot
            ? nextId()
            : Option.match(parent, {
                onNone: nextId,
                onSome: (p) => p.traceId,
              });
          const attributes = new Map<string, unknown>(
            Object.entries(options?.attributes ?? {})
          );
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
            attribute(key, value) {
              attributes.set(key, value);
            },
            event() {
              /* span events are not persisted */
            },
            addLinks() {
              /* links are not persisted */
            },
            end(endTime, exit) {
              const durationMs = Number(endTime - startTime) / NS_PER_MS;
              if (opts.echo) {
                writeConsoleSpan(name, durationMs, exit, attributes);
              }
              if (isRoot) {
                flushRoot({ name, traceId, durationMs, attributes, exit });
                return;
              }
              bufferChild(traceId, {
                name,
                durationMs,
                attributes: Object.fromEntries(attributes),
              });
            },
          };
          return span;
        },
        context: (f) => f(),
      });
      return Layer.setTracer(tracer);
    })
  );
