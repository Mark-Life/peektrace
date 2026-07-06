import { Schema } from "effect";

const Attributes = Schema.Record({
  key: Schema.String,
  value: Schema.Unknown,
});

const SpanRecord = Schema.Struct({
  name: Schema.String,
  durationMs: Schema.Number,
  attributes: Attributes,
});

const EventKind = Schema.Literal("cli", "rpc");

const ErrorInfo = Schema.Struct({
  tag: Schema.String,
  message: Schema.String,
  fields: Schema.optional(Attributes),
});
const DefectInfo = Schema.Struct({ message: Schema.String });

const baseFields = {
  id: Schema.String,
  traceId: Schema.String,
  ts: Schema.Number,
  kind: EventKind,
  name: Schema.String,
  appVersion: Schema.String,
  platform: Schema.String,
  durationMs: Schema.Number,
  attributes: Attributes,
  spans: Schema.Array(SpanRecord),
};

export const WideEvent = Schema.Union(
  Schema.Struct({ ...baseFields, outcome: Schema.Literal("success") }),
  Schema.Struct({
    ...baseFields,
    outcome: Schema.Literal("error"),
    error: ErrorInfo,
  }),
  Schema.Struct({
    ...baseFields,
    outcome: Schema.Literal("defect"),
    error: DefectInfo,
  })
);
export type WideEvent = typeof WideEvent.Type;
