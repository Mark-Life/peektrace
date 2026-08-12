/** The conversation reading of a transcript.
 *
 * Same filtered rows the table gets, arranged the way the session actually
 * happened: prompts on the right, model work on the left, everything injected
 * into the window centred between them. `buildChatPlan` decides the shape; this
 * file only maps nodes to components — and resolves each tool's name and outcome
 * here rather than inside a card, so a memoized card compares two strings
 * instead of the whole tool index, which is rebuilt on every re-analysis.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import { fmt, fmtK, PERCENT } from "@workspace/viz/lib/session-format";
import { FileTextIcon, ScissorsIcon } from "lucide-react";
import { useMemo } from "react";
import {
  buildChatPlan,
  type ChatNode,
  laneOf,
  type TranscriptRowRef,
} from "../../lib/chat-lane";
import type { ToolMarks } from "../../lib/session-markers";
import { chatCollapseId } from "../../lib/session-view";
import type { ToolIndex } from "../../lib/tool-event";
import { toolNameOf, toolState } from "../../lib/tool-event";
import { ChatContextRun } from "./chat-context-run";
import {
  ChatBand,
  ChatBubble,
  ChatThinking,
  ChatTurnRule,
  RawToggle,
} from "./chat-message";
import { ChatToolCard } from "./chat-tool-card";

/** Off-screen nodes skip layout; the transcript is long and mostly unread. */
const NODE = "[content-visibility:auto] [contain-intrinsic-size:auto_80px]";

/** A stable React key per node — position-based, like every collapse id here. */
const keyOf = (n: ChatNode, i: number) => {
  if (n.type === "message" || n.type === "band") {
    return `n${n.row.pos}`;
  }
  if (n.type === "tool") {
    return `n${n.call?.pos ?? n.result?.pos ?? i}`;
  }
  if (n.type === "run") {
    return `run${n.items[0]?.pos ?? i}`;
  }
  return `${n.type}${i}`;
};

/** A compaction replaced everything above it; a summary only adds to it. */
const Band = ({
  onToggle,
  open,
  row,
}: {
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  readonly row: TranscriptRowRef;
}) => {
  const { e, pos } = row;
  const id = chatCollapseId(pos);
  const compaction = e.kind === "compaction";
  const Icon = compaction ? ScissorsIcon : FileTextIcon;
  return (
    <ChatBand tone={compaction ? "warn" : "muted"}>
      <div
        data-kind={e.kind}
        data-lane="center"
        data-sidechain={e.isSidechain ? "true" : "false"}
        data-testid="history-event"
        id={id}
      >
        <button
          className="flex w-full items-center justify-center gap-1.5"
          onClick={() => onToggle(id, !open)}
          type="button"
        >
          <Icon className="size-3.5 shrink-0" />
          <span>
            {compaction
              ? "context compacted — everything above was replaced by this summary"
              : "rolling summary"}{" "}
            · ~{fmtK(e.tokensEst)}
          </span>
        </button>
        <p className="truncate text-muted-foreground text-xs">{e.preview}</p>
        {open ? (
          <div className="mt-1 flex flex-col gap-1 text-left">
            <div className="wrap-break-word max-h-[28rem] overflow-auto whitespace-pre-wrap text-xs">
              {e.body}
            </div>
            <RawToggle body={e.body} />
          </div>
        ) : null}
      </div>
    </ChatBand>
  );
};

/** No marks on this call: one shared empty array keeps the card memo stable. */
const NO_MARKS: readonly SessionMarker[] = [];

/** One node of the plan. Split out of the map so the switch stays readable. */
const renderNode = ({
  a,
  activeMark,
  allOpen,
  isOpen,
  marks,
  n,
  onToggle,
  queryActive,
  tools,
}: {
  readonly a: AnalyzedSession;
  readonly activeMark: string | null;
  readonly allOpen: boolean;
  readonly isOpen: (id: string) => boolean;
  readonly marks: ToolMarks;
  readonly n: ChatNode;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly queryActive: boolean;
  readonly tools: ToolIndex;
}) => {
  if (n.type === "dumbzone") {
    return (
      <ChatBand tone="danger">
        <span data-testid="dumbzone-divider">
          entered dumb zone — {Math.round(a.dumbZoneFraction * PERCENT)}% (
          {fmt(a.dumbZoneFraction * a.contextWindow)} tok) crossed at turn{" "}
          {a.dumbZoneCrossTurn + 1}
        </span>
      </ChatBand>
    );
  }
  if (n.type === "turn") {
    const turn = a.turns[n.turn - 1];
    if (!turn) {
      return null;
    }
    return (
      <ChatTurnRule
        ctxFrac={a.contextWindow > 0 ? turn.contextTokens / a.contextWindow : 0}
        dumbZone={a.dumbZoneFraction}
        model={turn.model}
        tokens={turn.contextTokens}
        turn={n.turn}
      />
    );
  }
  if (n.type === "run") {
    return (
      <ChatContextRun
        allOpen={allOpen}
        forceExpand={queryActive}
        isOpen={isOpen}
        items={n.items}
        onToggle={onToggle}
      />
    );
  }
  if (n.type === "band") {
    return (
      <Band
        onToggle={onToggle}
        open={isOpen(chatCollapseId(n.row.pos))}
        row={n.row}
      />
    );
  }
  if (n.type === "tool") {
    const anchor = n.call ?? n.result;
    if (!anchor) {
      return null;
    }
    const id = chatCollapseId(anchor.pos);
    const toolUseId = anchor.e.toolUseId;
    return (
      <ChatToolCard
        activeMark={activeMark}
        call={n.call?.e ?? null}
        collapseId={id}
        marks={(toolUseId ? marks.get(toolUseId) : null) ?? NO_MARKS}
        name={toolNameOf(anchor.e, tools)}
        onToggle={onToggle}
        open={isOpen(id)}
        result={n.result?.e ?? null}
        state={toolState(anchor.e, tools)}
      />
    );
  }
  const { e, pos } = n.row;
  const id = chatCollapseId(pos);
  const lane = laneOf(e) === "user" ? "user" : "agent";
  if (e.kind === "assistant-thinking") {
    return (
      <ChatThinking
        collapseId={id}
        e={e}
        lane={lane}
        onToggle={onToggle}
        open={isOpen(id)}
      />
    );
  }
  return (
    <ChatBubble
      collapseId={id}
      e={e}
      lane={lane}
      onToggle={onToggle}
      open={isOpen(id)}
    />
  );
};

/** The transcript as a conversation. */
export const TranscriptChat = ({
  a,
  activeMark,
  allOpen,
  crossEvtIdx,
  isOpen,
  marks,
  onToggle,
  queryActive,
  rows,
  tools,
  turns,
}: {
  readonly a: AnalyzedSession;
  /** The detector id the reader arrived with, highlighted where it applies. */
  readonly activeMark: string | null;
  readonly allOpen: boolean;
  readonly crossEvtIdx: number;
  readonly isOpen: (id: string) => boolean;
  /** Stats marks for this session, keyed by tool-use id. */
  readonly marks: ToolMarks;
  readonly onToggle: (id: string, open: boolean) => void;
  /** A search is running: chip clusters must not fold a match out of sight. */
  readonly queryActive: boolean;
  readonly rows: readonly TranscriptRowRef[];
  readonly tools: ToolIndex;
  readonly turns: readonly number[];
}) => {
  const plan = useMemo(
    () => buildChatPlan({ crossEvtIdx, rows, turns }),
    [crossEvtIdx, rows, turns]
  );

  if (rows.length === 0) {
    return (
      <p className="px-3 py-6 text-center text-muted-foreground text-sm">
        No events match the filters.
      </p>
    );
  }

  return (
    <div
      className="mx-auto flex w-full max-w-3xl flex-col gap-3"
      data-testid="chat-transcript"
    >
      {plan.map((n, i) => (
        <div className={NODE} key={keyOf(n, i)}>
          {renderNode({
            a,
            activeMark,
            allOpen,
            isOpen,
            marks,
            n,
            onToggle,
            queryActive,
            tools,
          })}
        </div>
      ))}
    </div>
  );
};
