/** Pure building blocks shared by the session export renderers.
 *
 * No `effect` and no `node:*` imports: the web inspector bundles these files.
 */
import { contextEditNote, eventBadgeLabel, thinkingNote } from "../labels";
import type { ParsedSession, TimelineEvent } from "../schema";

export const EXPORT_FORMATS = ["html", "md", "json"] as const;
export type ExportFormat = (typeof EXPORT_FORMATS)[number];

export interface ExportOptions {
  /** ISO time stamped into the output; passed in so renderers stay pure. */
  readonly exportedAt: string;
  /** The input was already redacted; home paths are then collapsed to `~`. */
  readonly redacted: boolean;
}

const HOME_PATH =
  /(?<![\w/])\/(?:home|Users)\/[^/\s"'`<>|:;,)]+|(?<!\w)[A-Za-z]:(?:\\{1,2}|\/)Users(?:\\{1,2}|\/)[^\\/\s"'`<>|:;,)]+/g;

/** Replace the home directory prefix (`/home/<user>`, `/Users/<user>`,
 * `C:\Users\<user>`) with `~`, anywhere in the text. Machine independent. */
export const collapseHomePaths = (text: string) => text.replace(HOME_PATH, "~");

const MIN_FENCE = 3;
const BACKTICK_RUN = /`+/g;

/** A backtick fence longer than any backtick run inside `body`. */
export const fenceFor = (body: string) => {
  let longest = 0;
  for (const run of body.match(BACKTICK_RUN) ?? []) {
    longest = Math.max(longest, run.length);
  }
  return "`".repeat(Math.max(MIN_FENCE, longest + 1));
};

export interface ExportEvent {
  readonly body: string;
  readonly contextEdit?:
    | {
        readonly status: "removed" | "replaced";
        readonly original?: string | undefined;
      }
    | undefined;
  readonly index: number;
  readonly isError?: boolean | undefined;
  readonly isMeta?: boolean | undefined;
  readonly isSidechain?: boolean | undefined;
  readonly kind: TimelineEvent["kind"];
  readonly label: string;
  /** Explains a context edit or missing thinking text. */
  readonly note?: string | undefined;
  readonly promptOrigin?: TimelineEvent["promptOrigin"];
  /** `user` and `assistant` render as chat messages, the rest collapsed. */
  readonly role: "user" | "assistant" | "other";
  readonly title: string;
  readonly tokensEst: number;
  readonly tokensMeasured?: number | undefined;
  readonly toolName?: string | undefined;
  readonly toolUseId?: string | undefined;
  readonly ts?: string | undefined;
}

export interface ExportSubagent {
  readonly agentType?: string | undefined;
  readonly description?: string | undefined;
  readonly id: string;
  readonly peakContextTokens: number;
  readonly turns: number;
  readonly unreadable?: true | undefined;
  readonly workflowPhase?: string | undefined;
}

export interface ExportDoc {
  readonly cwd?: string | undefined;
  readonly endedAt?: string | undefined;
  readonly events: readonly ExportEvent[];
  readonly exportedAt: string;
  readonly gitBranch?: string | undefined;
  readonly models: readonly string[];
  readonly provider: ParsedSession["provider"];
  readonly redacted: boolean;
  readonly sessionId: string;
  readonly startedAt?: string | undefined;
  readonly subagents: readonly ExportSubagent[];
  readonly title: string;
}

const ROLE_BY_KIND = {
  "user-prompt": "user",
  "assistant-text": "assistant",
} as const;

const editOriginal = (e: TimelineEvent) =>
  e.contextEdit?.status === "replaced" ? e.contextEdit.original : undefined;

const prepareEvent = (
  raw: TimelineEvent,
  clean: (text: string) => string
): ExportEvent | undefined => {
  const edit = raw.contextEdit;
  const e: TimelineEvent = {
    ...raw,
    title: clean(raw.title),
    preview: clean(raw.preview),
    body: clean(raw.body),
    ...(edit?.status === "replaced" && edit.original !== undefined
      ? { contextEdit: { ...edit, original: clean(edit.original) } }
      : {}),
  };
  const body = e.body === "" ? e.preview : e.body;
  if (body === "") {
    return;
  }
  const note =
    contextEditNote(e) ??
    (e.kind === "assistant-thinking" && e.body === ""
      ? thinkingNote(e)
      : undefined);
  const origin = e.promptOrigin;
  return {
    index: e.index,
    kind: e.kind,
    ts: e.ts,
    role: ROLE_BY_KIND[e.kind as keyof typeof ROLE_BY_KIND] ?? "other",
    label: eventBadgeLabel(e),
    title: e.title,
    body,
    note,
    toolName: e.toolName,
    toolUseId: e.toolUseId,
    isError: e.isError,
    isMeta: e.isMeta,
    isSidechain: e.isSidechain,
    promptOrigin: origin,
    tokensEst: e.tokensEst,
    tokensMeasured: e.tokensMeasured,
    contextEdit: edit
      ? { status: edit.status, original: editOriginal(e) }
      : undefined,
  };
};

/** Reduce a parsed session to the whitelisted, display-ready export shape.
 * Drops transcript paths and image refs; skips events with no text. */
export const prepareExport = (
  session: ParsedSession,
  { exportedAt, redacted }: ExportOptions
): ExportDoc => {
  const clean = redacted ? collapseHomePaths : (text: string) => text;
  const events = session.events.flatMap((raw) => {
    const e = prepareEvent(raw, clean);
    return e ? [e] : [];
  });
  return {
    sessionId: session.sessionId,
    title: clean(session.title ?? session.sessionId),
    provider: session.provider,
    models: session.models,
    gitBranch: session.gitBranch,
    cwd: session.cwd === undefined ? undefined : clean(session.cwd),
    startedAt: session.startedAt,
    endedAt: session.endedAt,
    exportedAt,
    redacted,
    events,
    subagents: session.subagents.map((s) => ({
      id: s.id,
      agentType: s.agentType,
      description:
        s.description === undefined ? undefined : clean(s.description),
      turns: s.turns,
      peakContextTokens: s.peakContextTokens,
      workflowPhase:
        s.workflowPhase === undefined ? undefined : clean(s.workflowPhase),
      unreadable: s.unreadable,
    })),
  };
};

export const LABEL_MAX = 80;
export const TITLE_MAX = 200;
export const NOTE_MAX = 400;
const LINE_MAX = 120;
const WHITESPACE = /\s+/g;

/** One-line text: whitespace runs become single spaces, long text is cut. */
export const oneLine = (text: string, max = LINE_MAX) => {
  const flat = text.replace(WHITESPACE, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
};

/** Header rows (label, value) shared by the Markdown and HTML renderers. */
export const metaRows = (d: ExportDoc) =>
  [
    ["Session", d.sessionId],
    ["Provider", d.provider],
    ["Models", d.models.join(", ")],
    ["Branch", d.gitBranch],
    ["Directory", d.cwd],
    ["Started", d.startedAt],
    ["Ended", d.endedAt],
    [
      "Exported",
      `${d.exportedAt} (${d.redacted ? "redacted" : "not redacted"})`,
    ],
  ].flatMap(([label, value]) =>
    label === undefined || value === undefined || value === ""
      ? []
      : [[label, oneLine(value, NOTE_MAX)] as const]
  );

/** One-line summary of a subagent: type, description, phase, size. */
export const subagentSummary = (s: ExportSubagent) =>
  [
    s.agentType,
    s.description === undefined ? undefined : oneLine(s.description),
    s.workflowPhase === undefined
      ? undefined
      : `phase ${oneLine(s.workflowPhase)}`,
    s.unreadable
      ? "unreadable"
      : `${s.turns} turns, peak ${s.peakContextTokens} tok`,
  ]
    .filter((p) => p !== undefined && p !== "")
    .join(", ");

/** Who spoke: `User`/`Assistant` for chat, the badge label for the rest. */
export const whoOf = (e: ExportEvent) => {
  if (e.role === "assistant") {
    return "Assistant";
  }
  return e.role === "user" && e.label === e.kind ? "User" : e.label;
};

/** Time and flags shown after the speaker, or an empty string. */
export const tailOf = (e: ExportEvent) =>
  [
    e.ts,
    e.isMeta ? "meta" : "",
    e.isSidechain ? "sidechain" : "",
    e.isError ? "error" : "",
  ]
    .filter((x) => x !== undefined && x !== "")
    .join(" · ");
