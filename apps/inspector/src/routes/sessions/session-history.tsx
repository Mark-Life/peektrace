/** Full collapsible history + subagents (Phase 8.3).
 *
 * Every transcript event in order (tool calls, results, attachments, assistant
 * text) collapsed by default, with search + type filter and the dumb-zone
 * divider rendered inline at the first crossing. Subagent (sidechain) transcripts
 * drill down as their own cards. The transcript is REDACTED BY DEFAULT behind a
 * persistent "review before sharing" banner; the reveal toggle re-fetches with
 * `redact:false` (handled by the parent atom).
 */
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import {
  CodeBlock,
  CodeBlockCopyButton,
} from "@workspace/ui/components/ai-elements/code-block";
import { Badge } from "@workspace/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import { Input } from "@workspace/ui/components/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { Switch } from "@workspace/ui/components/switch";
import { cn } from "@workspace/ui/lib/utils";
import { fmt, fmtK, PERCENT } from "@workspace/viz/lib/session-format";
import { ChevronRightIcon, ShieldAlertIcon } from "lucide-react";
import { useMemo, useState } from "react";

/** Event-kind options for the history type filter. */
const KIND_OPTIONS = [
  { value: "all", label: "All types" },
  { value: "user-prompt", label: "User" },
  { value: "assistant-text", label: "Assistant" },
  { value: "tool-call", label: "Tool calls" },
  { value: "tool-result", label: "Tool results" },
  { value: "assistant-thinking", label: "Thinking" },
  { value: "attachment", label: "Attachments" },
] as const;

/** Map every event to the 1-based turn it belongs to (for grouping). */
const turnNumbers = (a: AnalyzedSession): number[] => {
  const reqToTurn = new Map(
    a.turns.map((t, i) => [t.requestId, i + 1] as const)
  );
  const out: number[] = [];
  let cur = 0;
  for (const e of a.events) {
    if (e.requestId && reqToTurn.has(e.requestId)) {
      cur = reqToTurn.get(e.requestId) ?? cur;
    }
    out.push(cur);
  }
  return out;
};

/** Languages we highlight transcript bodies as (subset of shiki bundled langs). */
type BodyLang = "typescript" | "bash" | "json" | "markdown";

const parseJson = (s: string): unknown => {
  try {
    return JSON.parse(s);
  } catch {
    return;
  }
};

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Join `{ type: "text", text }` blocks (standard tool_result content). */
const textFromBlocks = (v: unknown): string | null => {
  if (!Array.isArray(v)) {
    return null;
  }
  const texts = v
    .filter(
      (b): b is { text: string } =>
        isRecord(b) && b.type === "text" && typeof b.text === "string"
    )
    .map((b) => b.text);
  return texts.length > 0 ? texts.join("\n\n") : null;
};

/**
 * Turn a raw event body into a syntax-highlightable block, unwrapping
 * JSON-encoded tool payloads so escaped newlines render as real lines
 * (e.g. an executor `code` arg or a JSON result string). Returns null to
 * fall back to plain <pre> for prose, thinking, and non-JSON errors.
 */
const displayBody = (
  e: TimelineEvent
): { code: string; language: BodyLang } | null => {
  if (e.kind === "tool-call") {
    const input = parseJson(e.body);
    if (isRecord(input)) {
      if (typeof input.code === "string") {
        return { code: input.code, language: "typescript" };
      }
      if (typeof input.command === "string") {
        return { code: input.command, language: "bash" };
      }
    }
    return { code: e.body, language: "json" };
  }
  if (e.kind === "tool-result") {
    const parsed = parseJson(e.body);
    if (parsed === undefined) {
      return null;
    }
    const text = textFromBlocks(parsed);
    if (text !== null) {
      return { code: text, language: "markdown" };
    }
    return { code: JSON.stringify(parsed, null, 2), language: "json" };
  }
  return null;
};

/** One collapsible transcript event. */
const EventRow = ({
  e,
  turn,
}: {
  readonly e: TimelineEvent;
  readonly turn: number;
}) => {
  const hasBody = e.body.trim().length > 0;
  const view = hasBody ? displayBody(e) : null;
  const emptyText =
    e.kind === "assistant-thinking"
      ? "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band."
      : "(no content)";
  return (
    <Collapsible
      className="border-border border-b"
      data-kind={e.kind}
      data-sidechain={e.isSidechain ? "true" : "false"}
      data-testid="history-event"
    >
      <CollapsibleTrigger className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-sm hover:bg-muted/40 [&[data-state=open]>svg]:rotate-90">
        <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform" />
        <span className="w-8 shrink-0 font-mono text-muted-foreground text-xs">
          t{turn}
        </span>
        <Badge className="shrink-0" variant="outline">
          {e.toolName ?? e.kind}
        </Badge>
        {e.isSidechain ? (
          <Badge className="shrink-0" variant="secondary">
            sidechain
          </Badge>
        ) : null}
        <span className="truncate text-muted-foreground text-xs">
          {e.preview || "(empty)"}
        </span>
        <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
          {e.tokensEst ? `~${fmt(e.tokensEst)}` : ""}
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        {view ? (
          <div className="max-h-96 overflow-auto">
            <CodeBlock
              className="[&_pre]:whitespace-pre-wrap! [&_pre]:wrap-break-word! rounded-none border-0 border-t [&_code]:text-[11px]! [&_pre]:p-3! [&_pre]:text-[9px]! [&_pre]:leading-relaxed!"
              code={view.code}
              language={view.language}
            >
              <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
            </CodeBlock>
          </div>
        ) : (
          <pre className="wrap-break-word max-h-96 overflow-auto whitespace-pre-wrap bg-muted/30 px-3 py-2 text-xs">
            {hasBody ? e.body : emptyText}
          </pre>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
};

/** Subagent (sidechain) transcript cards — each runs in its own window. */
const Subagents = ({ a }: { readonly a: AnalyzedSession }) => {
  if (a.subagents.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-2" data-testid="subagents">
      <h3 className="font-medium text-sm">Subagents ({a.subagents.length})</h3>
      <p className="text-muted-foreground text-xs">
        Each runs in its own context window — these tokens do not count against
        the main session.
      </p>
      {a.subagents.map((s) => (
        <Collapsible
          className="rounded-md border border-border"
          data-testid="subagent-card"
          key={s.id}
        >
          <CollapsibleTrigger className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-muted/40 [&[data-state=open]>svg]:rotate-90">
            <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform" />
            <Badge variant="secondary">{s.agentType ?? "agent"}</Badge>
            <span className="font-mono text-xs">{s.id}</span>
            <span className="truncate text-muted-foreground text-xs">
              {s.description ?? "subagent"} · {s.turns} turns
            </span>
            <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
              peak {fmtK(s.peakContextTokens)}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <pre className="whitespace-pre-wrap break-words bg-muted/30 px-3 py-2 text-xs">
              {`agentType: ${s.agentType ?? "—"}
description: ${s.description ?? "—"}
toolUseId: ${s.toolUseId ?? "—"}
turns: ${s.turns}
peak context: ${fmt(s.peakContextTokens)}
path: ${s.path}`}
            </pre>
          </CollapsibleContent>
        </Collapsible>
      ))}
    </div>
  );
};

/** Full history section with filters, redaction banner, and subagent drill-down. */
export const SessionHistory = ({
  a,
  redacted,
  onToggleRedact,
}: {
  readonly a: AnalyzedSession;
  readonly redacted: boolean;
  readonly onToggleRedact: (next: boolean) => void;
}) => {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  const turns = useMemo(() => turnNumbers(a), [a]);

  const crossEvtIdx =
    a.dumbZoneCrossTurn >= 0
      ? a.events.findIndex(
          (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
        )
      : -1;

  const visible = useMemo(
    () =>
      a.events
        .map((e, pos) => ({ e, pos }))
        .filter(({ e }) => e.kind !== "system")
        .filter(({ e }) => kind === "all" || e.kind === kind)
        .filter(({ e }) => {
          if (query.length === 0) {
            return true;
          }
          const hay = `${e.title} ${e.preview}`.toLowerCase();
          return hay.includes(query.toLowerCase());
        }),
    [a.events, kind, query]
  );

  return (
    <section className="flex flex-col gap-3" data-testid="session-history">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-semibold text-base">Full history</h2>
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground" id="redact-toggle-label">
            Reveal secrets
          </span>
          <Switch
            aria-labelledby="redact-toggle-label"
            checked={!redacted}
            data-testid="redact-toggle"
            onCheckedChange={(checked) => onToggleRedact(!checked)}
          />
        </div>
      </div>

      <div
        className={cn(
          "flex items-center gap-2 rounded-md border px-3 py-2 text-destructive text-xs",
          redacted
            ? "border-destructive/30 bg-destructive/5"
            : "border-destructive/60 bg-destructive/15 font-medium"
        )}
        data-testid="redaction-banner"
      >
        <ShieldAlertIcon className="size-4 shrink-0" />
        {redacted ? (
          <span>
            Secrets are redacted by default. This transcript may still contain
            sensitive data — review before sharing.
          </span>
        ) : (
          <span data-testid="redaction-off">
            Redaction is OFF — the raw transcript (including secrets) is shown.
            Do not share.
          </span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <Input
          className="max-w-xs"
          data-testid="history-search"
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search history…"
          value={query}
        />
        <Select onValueChange={setKind} value={kind}>
          <SelectTrigger className="w-40" data-testid="history-kind-filter">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {KIND_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-muted-foreground text-xs">
          {visible.length} events · {a.dumbZoneTurns}/{a.turnCount} turns in
          dumb zone
        </span>
      </div>

      <div className="rounded-md border border-border">
        {visible.map(({ e, pos }) => (
          <div key={`${e.index}-${pos}`}>
            {pos === crossEvtIdx ? (
              <div
                className="bg-red-500/15 px-3 py-1.5 text-center font-medium text-red-300 text-xs"
                data-testid="dumbzone-divider"
              >
                entered dumb zone — {Math.round(a.dumbZoneFraction * PERCENT)}%
                ({fmt(a.dumbZoneFraction * a.contextWindow)} tok) crossed at
                turn {a.dumbZoneCrossTurn + 1}
              </div>
            ) : null}
            <EventRow e={e} turn={turns[pos] ?? 0} />
          </div>
        ))}
        {visible.length === 0 ? (
          <p className="px-3 py-6 text-center text-muted-foreground text-sm">
            No events match the filters.
          </p>
        ) : null}
      </div>

      <Subagents a={a} />
    </section>
  );
};
