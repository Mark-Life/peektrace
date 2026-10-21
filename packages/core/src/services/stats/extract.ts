/**
 * One parsed session in, one array of facts out.
 *
 * The join is the only part of the skill's scanner that survives: calls are held
 * in a `Map` keyed on tool-use id, matched to the first result that names them,
 * and whatever is left when the transcript ends is flushed as a call with no
 * result. `seq` is a counter assigned here, at emit time — line order is not a
 * clock, because a row is emitted when its result parses while `ts` is the call
 * time, and 1.3% of adjacent within-session pairs otherwise go backwards.
 *
 * Every derived string is redacted here, where it is minted, so a shard on disk
 * holds no unredacted text. Detector calls happen here too, while the result
 * text is still in hand: the fact stores `failed` + `failReason`, never the
 * output, so a later cap on stored text cannot lose a failure.
 */
import type { ParsedSession, TimelineEvent } from "../sessions/schema";
import {
  CAUSES,
  type CauseHit,
  detectCause,
  SHELL_TOOLS,
} from "./detectors/causes";
import { errorSignature, normalizeCommand } from "./detectors/clustering";
import { benignFilter, isPiped, scanOutput } from "./detectors/failures";
import { FOCUS_SEP } from "./detectors/similarity";
import { classifyJob, isBackgrounded, isCapped } from "./detectors/timing";
import { declaredDelaySec, waitShape } from "./detectors/waiting";
import { round } from "./rank";
import type { Redactor } from "./redact";
import type { BashFact, SessionFact, StatsFact, ToolFact } from "./schema";
import type { TranscriptRef } from "./walk";

/** Chars of command text a fact keeps, after redaction. */
const CMD_MAX = 400;

/** Chars of a primary argument a fact keeps, after redaction. */
const ARG_MAX = 200;

/** Chars of a cause culprit a fact keeps, after redaction. */
const CAUSE_ARG_MAX = 80;

/** Chars of one failing segment a fact keeps, after redaction. */
const FOCUS_MAX = 200;

/** Failing segments a fact keeps. */
const FOCUS_SEGMENTS = 3;

/** Decimal places a duration is stored with, so shard bytes are stable. */
const DUR_DIGITS = 3;

const MS_PER_SEC = 1000;

/** Per-tool primary argument, in the order the field names are tried. */
const ARG_KEYS = [
  "file_path",
  "path",
  "pattern",
  "url",
  "query",
  "subagent_type",
  "notebook_path",
  "command",
  // Codex's exec_command line.
  "cmd",
] as const;

/** The `-lc` / `-c` flag of an argv-style shell call, whose payload is the line. */
const SHELL_FLAG = /^-{1,2}[lc]{1,2}$/;

/** A call waiting for its result. */
interface PendingCall {
  readonly id: string | null;
  readonly input: string;
  /** Request id of the model call that emitted this call. */
  readonly req: string | null;
  readonly side: boolean;
  readonly tool: string;
  readonly ts: string | null;
}

const asObject = (text: string): Record<string, unknown> | null => {
  if (!text.startsWith("{")) {
    return null;
  }
  try {
    const parsed = JSON.parse(text) as unknown;
    return typeof parsed === "object" && parsed !== null
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
};

/**
 * The command a shell call ran. Claude and OpenCode pass a string; Codex passes
 * an argv array, usually `["bash","-lc","<line>"]`, whose payload is the line.
 */
const commandOf = (input: Record<string, unknown> | null): string => {
  const raw = input?.command;
  if (typeof raw === "string") {
    return raw;
  }
  if (!Array.isArray(raw)) {
    return "";
  }
  const parts = raw.filter((p): p is string => typeof p === "string");
  const last = parts.at(-1) ?? "";
  return parts.length > 1 && SHELL_FLAG.test(parts[1] ?? "")
    ? last
    : parts.join(" ");
};

/** The tool's primary argument, or null when it has none worth keying on. */
const argOf = (input: Record<string, unknown> | null): string | null => {
  for (const key of ARG_KEYS) {
    const value = input?.[key];
    if (typeof value === "string" && value !== "") {
      return value;
    }
  }
  return null;
};

/** Seconds between a call and its result. Negative clock skew clamps to 0. */
const durationOf = (call: string | null, result: string | null) => {
  if (!(call && result)) {
    return null;
  }
  const a = Date.parse(call);
  const b = Date.parse(result);
  if (Number.isNaN(a) || Number.isNaN(b)) {
    return null;
  }
  return round(Math.max(0, (b - a) / MS_PER_SEC), DUR_DIGITS);
};

/** Fields shared by the session row and every call row of one transcript. */
const identityOf = (args: {
  readonly ref: TranscriptRef;
  readonly project: string;
}) => ({
  sid: args.ref.id,
  agent: args.ref.agent,
  project: args.project,
  kind: args.ref.kind,
  parent: args.ref.parent,
  run: args.ref.run,
});

type Identity = ReturnType<typeof identityOf>;

/** The transcript summary row: token totals and the fan-out prefix cost. */
const sessionFactOf = (args: {
  readonly identity: Identity;
  readonly parsed: ParsedSession;
  readonly redact: Redactor;
  /** The agent source the transcript is stored in, or null for the default. */
  readonly source: string | null;
  readonly toolCalls: number;
}): SessionFact => {
  const { identity, parsed, redact, source, toolCalls } = args;
  const totals = parsed.turns.reduce(
    (acc, turn) => ({
      inputTokens: acc.inputTokens + turn.inputTokens,
      outputTokens: acc.outputTokens + turn.outputTokens,
      cacheReadTokens: acc.cacheReadTokens + turn.cacheReadTokens,
      cacheCreationTokens: acc.cacheCreationTokens + turn.cacheCreationTokens,
      maxContextTokens: Math.max(acc.maxContextTokens, turn.contextTokens),
    }),
    {
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      maxContextTokens: 0,
    }
  );
  return {
    row: "session",
    ...identity,
    startedAt: parsed.startedAt ?? null,
    endedAt: parsed.endedAt ?? null,
    title: redact.nullable(parsed.title),
    cwd: redact.nullable(parsed.cwd),
    source,
    models: parsed.models,
    turns: parsed.turns.length,
    userMessages: parsed.events.filter((e) => e.kind === "user-prompt").length,
    toolCalls,
    ...totals,
    firstTurnCacheCreation: parsed.turns[0]?.cacheCreationTokens ?? 0,
  };
};

/**
 * Clip each failing segment on its own, so a clip never splits two. The
 * detector already cut them from redacted text; this pass catches what the
 * normalised form still shows.
 */
const focusOf = (hit: CauseHit | null, redact: Redactor): string | null => {
  if (hit === null || hit.focus === null) {
    return null;
  }
  const parts = hit.focus
    .split(FOCUS_SEP)
    .slice(0, FOCUS_SEGMENTS)
    .map((s) => redact.line(s, FOCUS_MAX))
    .filter((s) => s !== "");
  return parts.length === 0 ? null : parts.join(FOCUS_SEP);
};

/** The error signature: a cause reads its own line, not the last one printed. */
const sigOf = (args: {
  readonly tool: string;
  readonly output: string;
  readonly hit: CauseHit | null;
  readonly failed: boolean;
}): string | null => {
  const { tool, output, hit, failed } = args;
  if (!failed) {
    return null;
  }
  const line = hit?.line ?? null;
  return errorSignature({ tool, text: line ?? output });
};

/** Build the tool or bash row for one joined call. */
const callFactOf = (args: {
  readonly identity: Identity;
  readonly seq: number;
  readonly call: PendingCall;
  readonly result: TimelineEvent | null;
  readonly redact: Redactor;
  readonly ctxTok: number | null;
}): ToolFact | BashFact => {
  const { identity, seq, call, result, redact, ctxTok } = args;
  const output = result?.body ?? "";
  const err = result?.isError === true;
  const scan = scanOutput({ tool: call.tool, text: output, isError: err });
  const input = asObject(call.input);
  const arg = argOf(input);
  const shell = SHELL_TOOLS.has(call.tool);
  const cmd = shell ? commandOf(input) : "";
  const hit = detectCause({ tool: call.tool, cmd, output, scrub: redact.text });
  // A failing cause is a failure whatever the exit code said: a glob abort in
  // a chain segment exits 0 when the tail of the chain succeeds.
  const failed = scan.failed || (hit !== null && CAUSES[hit.cause].failing);
  const sig = sigOf({ tool: call.tool, output, hit, failed: failed || err });
  const base = {
    ...identity,
    seq,
    toolUseId: call.id,
    tool: call.tool,
    ts: call.ts,
    durSec: durationOf(call.ts, result?.ts ?? null),
    err,
    failed,
    failReason: scan.reason,
    detector: scan.detector,
    benign: null,
    sig: sig === null ? null : redact.line(sig),
    arg: arg === null ? null : redact.line(arg, ARG_MAX),
    outChars: output.length,
    side: call.side,
    req: call.req,
    ctxTok,
    cause: hit?.cause ?? null,
    causeArg:
      hit === null || hit.arg === null
        ? null
        : redact.line(hit.arg, CAUSE_ARG_MAX),
    focus: focusOf(hit, redact),
  };
  if (!shell) {
    return { row: "tool", ...base } satisfies ToolFact;
  }
  const { bin, family } = normalizeCommand(cmd);
  const { job, present } = classifyJob(cmd);
  return {
    ...base,
    row: "bash",
    benign: benignFilter({
      cmd,
      output,
      detector: scan.detector,
      cause: base.cause,
    }),
    cmd: redact.line(cmd, CMD_MAX),
    bin,
    family: redact.text(family),
    job,
    present,
    wait: waitShape(cmd),
    declaredDelaySec: round(declaredDelaySec(cmd)),
    piped: isPiped(cmd),
    // The harness flag is the larger half of the signal: over the live corpus it
    // marks 196 rows the command text alone cannot show.
    bg: isBackgrounded(cmd) || input?.run_in_background === true,
    capped: isCapped(base.durSec),
  } satisfies BashFact;
};

/**
 * Facts for one transcript: the session row first, then every joined call in
 * emit order. Nothing else in the service reads a `TimelineEvent`.
 */
export const extractFacts = (args: {
  readonly ref: TranscriptRef;
  readonly parsed: ParsedSession;
  /** Project key, already normalised to one form across agents. */
  readonly project: string;
  readonly redact: Redactor;
}): readonly StatsFact[] => {
  const { ref, parsed, project, redact } = args;
  const identity = identityOf({ ref, project });
  const pending = new Map<string, PendingCall>();
  const matched = new Set<string>();
  const calls: (ToolFact | BashFact)[] = [];
  // Context of the model call behind each tool call: what a retry re-read.
  // Sidechain turns are not registered, so their calls stay uncosted (null).
  const ctxByReq = new Map(
    parsed.turns.map((t) => [t.requestId, t.contextTokens] as const)
  );
  let seq = 0;

  const emit = (call: PendingCall, result: TimelineEvent | null) => {
    const ctxTok = call.req === null ? null : (ctxByReq.get(call.req) ?? null);
    calls.push(callFactOf({ identity, seq, call, result, redact, ctxTok }));
    seq += 1;
  };

  for (const event of parsed.events) {
    const id = event.toolUseId;
    if (event.kind === "tool-call" && id) {
      pending.set(id, {
        id,
        tool: event.toolName ?? "tool",
        input: event.body,
        req: event.requestId ?? null,
        ts: event.ts ?? null,
        side: event.isSidechain === true,
      });
      continue;
    }
    if (event.kind !== "tool-result" || !id || matched.has(id)) {
      continue;
    }
    const call = pending.get(id);
    if (call) {
      matched.add(id);
      pending.delete(id);
      emit(call, event);
    }
  }
  for (const call of pending.values()) {
    emit(call, null);
  }

  return [
    sessionFactOf({
      identity,
      parsed,
      redact,
      source: ref.source?.id ?? null,
      toolCalls: calls.length,
    }),
    ...calls,
  ];
};
