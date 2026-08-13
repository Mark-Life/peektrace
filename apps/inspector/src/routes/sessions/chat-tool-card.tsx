/** One tool run as a single card: the call and the result it got.
 *
 * The table keeps them apart because it is a forensics view; a conversation
 * reads as "asked, answered", so they pair here. The two token figures stay two
 * figures — `~in → ~out` — because summing them would invent a number the
 * transcript never recorded.
 */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import {
  Tool,
  ToolContent,
  ToolDiff,
  ToolHeader,
  ToolInput,
  ToolOutput,
  type ToolState,
} from "@workspace/ui/components/ai-elements/tool";
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import { fmt, fmtK } from "@workspace/viz/lib/session-format";
import { CornerDownLeftIcon } from "lucide-react";
import { memo } from "react";
import { callView, resultPane } from "../../lib/tool-event";
import { sameToolPair } from "../../lib/transcript-row";
import { ChatRow, RawToggle } from "./chat-message";

interface ChatToolCardProps {
  /** The detector id the reader arrived with, from `#/sessions?…&mark=`. */
  readonly activeMark: string | null;
  readonly call: TimelineEvent | null;
  readonly collapseId: string;
  /** Stats marks on this call, carrying the same detector ids the tables use. */
  readonly marks: readonly SessionMarker[];
  readonly name: string;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  readonly result: TimelineEvent | null;
  readonly state: ToolState;
}

/** Short glyph text per detector. The badge says what was printed, never that
 *  something broke — a silent failure is a failure the exit code did not
 *  report, which is not the same claim. */
const MARK_TEXT: Record<string, string> = {
  "zsh-equals-abort": "rule",
  "piped-exit-code": "silent fail",
  "silent-fail": "silent fail",
  capped: "capped",
  repeat: "repeat",
};

/** One stats marker, tinted amber and never red: a marked call is not an error
 *  the agent reported, and the error tint already means that. */
const MarkBadge = ({
  active,
  mark,
}: {
  readonly active: boolean;
  readonly mark: SessionMarker;
}) => (
  <Badge
    className={cn(
      "shrink-0 border-amber-500/40 bg-amber-500/10 text-amber-400",
      active && "ring-1 ring-amber-400"
    )}
    data-detector={mark.detector}
    data-testid="stats-marker"
    title={mark.label}
    variant="outline"
  >
    {MARK_TEXT[mark.detector] ?? mark.detector}
  </Badge>
);

/** `~1.2K → ~8K`, or a dash for whichever half the transcript does not have. */
const costPair = (
  call: TimelineEvent | null,
  result: TimelineEvent | null
) => ({
  short: `${call ? `~${fmtK(call.tokensEst)}` : "—"} → ${
    result ? `~${fmtK(result.tokensEst)}` : "—"
  }`,
  title: `call ${call ? `~${fmt(call.tokensEst)}` : "—"} tok → result ${
    result ? `~${fmt(result.tokensEst)}` : "—"
  } tok (never summed)`,
});

/** The raw bytes of both halves, so the panes' JSON parsing stays reversible. */
const rawBoth = (call: TimelineEvent | null, result: TimelineEvent | null) =>
  [
    call ? `— call —\n${call.body}` : null,
    result ? `— result —\n${result.body}` : null,
  ]
    .filter((s) => s !== null)
    .join("\n\n");

const ChatToolCardBody = ({
  activeMark,
  call,
  collapseId,
  marks,
  name,
  onToggle,
  open,
  result,
  state,
}: ChatToolCardProps) => {
  const view = call ? callView(call) : null;
  const out = result ? resultPane(result) : null;
  const anchor = call ?? result;
  const cost = costPair(call, result);
  return (
    <ChatRow lane="agent" sidechain={anchor?.isSidechain === true}>
      <Tool
        className={cn(
          "w-full max-w-[92%] rounded-xl border",
          result?.isError === true && "border-destructive/40"
        )}
        data-kind={call ? "tool-call" : "tool-result"}
        data-lane="agent"
        data-paired={call && result ? "true" : "false"}
        data-sidechain={anchor?.isSidechain ? "true" : "false"}
        data-testid="history-event"
        id={collapseId}
        onOpenChange={(next: boolean) => onToggle(collapseId, next)}
        open={open}
      >
        <ToolHeader
          className="px-3 py-2 text-sm hover:bg-muted/40"
          icon={
            call ? undefined : (
              <CornerDownLeftIcon className="size-3.5 shrink-0 text-muted-foreground" />
            )
          }
          name={name}
          state={state}
        >
          {call ? null : (
            <Badge className="shrink-0" variant="outline">
              result only
            </Badge>
          )}
          {anchor?.isSidechain ? (
            <Badge className="shrink-0" variant="secondary">
              sidechain
            </Badge>
          ) : null}
          {marks.map((mark) => (
            <MarkBadge
              active={activeMark === mark.detector}
              key={`${mark.detector}:${mark.seq}`}
              mark={mark}
            />
          ))}
          <span className="truncate text-muted-foreground text-xs">
            {(view ? view.summary : result?.preview) || "(empty)"}
          </span>
          <span
            className="ml-auto shrink-0 font-mono text-muted-foreground text-xs"
            title={cost.title}
          >
            {cost.short}
          </span>
        </ToolHeader>
        <ToolContent className="max-h-96 overflow-auto">
          {view?.panes.map((p) =>
            p.type === "diff" ? (
              <ToolDiff
                diff={p.diff}
                key={p.label}
                label={p.label}
                language={p.language}
                newCode={p.newCode}
                oldCode={p.oldCode}
              />
            ) : (
              <ToolInput
                code={p.code}
                key={p.label}
                label={p.label}
                language={p.language}
              />
            )
          )}
          {out ? (
            <ToolOutput
              code={out.code}
              isError={result?.isError === true}
              language={out.language}
            />
          ) : null}
          <div className="px-3 py-2">
            <RawToggle body={rawBoth(call, result)} />
          </div>
        </ToolContent>
      </Tool>
    </ChatRow>
  );
};

/** One tool card, re-rendered only when either half or its state changes. */
export const ChatToolCard = memo(ChatToolCardBody, sameToolPair);
