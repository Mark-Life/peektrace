/** The conversation reading of a transcript.
 *
 * Same filtered rows the table gets, arranged the way the session actually
 * happened: prompts on the right, model work on the left, everything injected
 * into the window — context, and prompts the harness wrote — centred between
 * them. `buildChatPlan` decides the shape; this
 * file only maps nodes to components — and resolves each tool's name and outcome
 * here rather than inside a card, so a memoized card compares two strings
 * instead of the whole tool index, which is rebuilt on every re-analysis.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { tokenLabel } from "@workspace/core/services/sessions/tokens";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import { Markdown } from "@workspace/ui/components/ai-elements/markdown";
import { cn } from "@workspace/ui/lib/utils";
import { fmt, fmtK, PERCENT } from "@workspace/viz/lib/session-format";
import { FileTextIcon, ScissorsIcon, SettingsIcon } from "lucide-react";
import { memo, type RefObject, useMemo, useRef } from "react";
import {
  buildChatPlan,
  type ChatNode,
  chatNodeKeys,
  laneOf,
  planIndexOfCall,
  type TranscriptRowRef,
} from "../../lib/chat-lane";
import {
  alignOf,
  type HistoryJump,
  planIndexOfTarget,
} from "../../lib/history-jump";
import type { ToolMarks } from "../../lib/session-markers";
import { chatCollapseId } from "../../lib/session-view";
import type { ToolIndex } from "../../lib/tool-event";
import { toolNameOf, toolState } from "../../lib/tool-event";
import {
  CHAT_ITEM_CLASS,
  estimateChatNode,
} from "../../lib/transcript-estimate";
import { useVirtualJump } from "../../lib/virtual-jump";
import type { VirtualListHandle } from "../../lib/virtual-range";
import { ChatContextRun } from "./chat-context-run";
import {
  ChatBand,
  ChatBubble,
  ChatThinking,
  ChatTurnRule,
  RawToggle,
} from "./chat-message";
import { ChatNotice } from "./chat-notice";
import { ChatToolCard } from "./chat-tool-card";
import { VirtualList } from "./virtual-list";

/** What a band says about its event: compaction and summary are fixed notices,
 *  a system prompt reads its own title (first prompt vs. a later update). */
const bandLabel = (e: TranscriptRowRef["e"]) => {
  if (e.kind === "compaction") {
    return "context compacted — everything above was replaced by this summary";
  }
  return e.kind === "summary" ? "rolling summary" : e.title.toLowerCase();
};

/** A system prompt body is `## section` blocks and, under tools, one `### tool`
 *  each: set the sections apart as labelled dividers and each tool on its own
 *  rule, so forty tool definitions do not run together. */
const PROMPT_SECTIONS =
  "[&_h2]:mt-5 [&_h2]:border-b [&_h2]:pb-1 [&_h2]:font-mono [&_h2]:text-[11px] [&_h2]:text-muted-foreground [&_h2]:uppercase [&_h2]:tracking-wide [&_h3]:mt-4 [&_h3]:border-t [&_h3]:border-dashed [&_h3]:pt-3 [&_h3]:font-mono [&_h3]:text-sm";

const BAND_ICON = {
  compaction: ScissorsIcon,
  "system-prompt": SettingsIcon,
} as const;

/** A compaction replaced everything above it; a summary only adds to it; a
 *  system prompt sits under everything and stays folded until asked for. */
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
  const Icon =
    e.kind === "compaction" || e.kind === "system-prompt"
      ? BAND_ICON[e.kind]
      : FileTextIcon;
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
            {bandLabel(e)} · {tokenLabel(e, fmtK)}
          </span>
        </button>
        <p className="truncate text-muted-foreground text-xs">{e.preview}</p>
        {open ? (
          <div className="mt-1 flex flex-col gap-1 text-left">
            <Markdown
              className={cn(
                "max-h-[28rem] overflow-auto text-xs",
                e.kind === "system-prompt" && PROMPT_SECTIONS
              )}
              text={e.body}
            />
            <RawToggle body={e.body} />
          </div>
        ) : null}
      </div>
    </ChatBand>
  );
};

/** No marks on this call: one shared empty array keeps the card memo stable. */
const NO_MARKS: readonly SessionMarker[] = [];

/** One node of the plan, as the transcript draws it. */
const ChatNodeViewBody = ({
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
        effort={turn.effort}
        model={turn.model}
        outputTokens={turn.outputTokens}
        thinkingTokens={turn.thinkingTokens}
        tokens={turn.contextTokens}
        turn={n.turn}
        webFetches={turn.webFetchRequests}
        webSearches={turn.webSearchRequests}
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
  if (n.type === "notice") {
    const id = chatCollapseId(n.row.pos);
    return (
      <ChatNotice
        collapseId={id}
        e={n.row.e}
        onToggle={onToggle}
        open={isOpen(id)}
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

export const ChatNodeView = memo(ChatNodeViewBody);

/** The transcript as a conversation. Only the blocks near the screen are
 *  mounted; `callId` is the tool call a link asked to land on. */
export const TranscriptChat = ({
  a,
  activeMark,
  allOpen,
  bulkKey,
  callId,
  chipDone,
  crossEvtIdx,
  isOpen,
  jump,
  jumpDone,
  marks,
  onToggle,
  queryActive,
  rows,
  stickyHeight,
  tools,
  turns,
}: {
  readonly a: AnalyzedSession;
  /** The detector id the reader arrived with, highlighted where it applies. */
  readonly activeMark: string | null;
  readonly allOpen: boolean;
  /** Bumped by expand or collapse all, when every height changes at once. */
  readonly bulkKey: number;
  readonly callId: string | null;
  /** The summary jump already landed on, shared by both layouts. */
  readonly chipDone: RefObject<string | null>;
  readonly crossEvtIdx: number;
  readonly isOpen: (id: string) => boolean;
  /** Where the summary asked to go, if anywhere. */
  readonly jump: HistoryJump | null;
  /** The call link already landed on, shared by both layouts. */
  readonly jumpDone: RefObject<string | null>;
  /** Stats marks for this session, keyed by tool-use id. */
  readonly marks: ToolMarks;
  readonly onToggle: (id: string, open: boolean) => void;
  /** A search is running: chip clusters must not fold a match out of sight. */
  readonly queryActive: boolean;
  readonly rows: readonly TranscriptRowRef[];
  /** Height of the sticky toolbar a jump must land below. */
  readonly stickyHeight: number;
  readonly tools: ToolIndex;
  readonly turns: readonly number[];
}) => {
  const plan = useMemo(
    () => buildChatPlan({ crossEvtIdx, rows, turns }),
    [crossEvtIdx, rows, turns]
  );
  const keys = useMemo(() => chatNodeKeys(plan), [plan]);
  const list = useRef<VirtualListHandle>(null);
  useVirtualJump({
    done: jumpDone,
    index: callId === null ? -1 : planIndexOfCall(plan, callId),
    list,
    target: callId,
  });
  useVirtualJump({
    align: jump ? alignOf(jump.target) : "start",
    done: chipDone,
    index: jump ? planIndexOfTarget(plan, jump.target) : -1,
    list,
    target: jump?.key ?? null,
  });

  if (rows.length === 0) {
    return (
      <p className="px-3 py-6 text-center text-muted-foreground text-sm">
        No events match the filters.
      </p>
    );
  }

  return (
    <VirtualList
      className="mx-auto w-full max-w-3xl"
      estimate={(n) => estimateChatNode({ allOpen, isOpen, n })}
      itemClassName={CHAT_ITEM_CLASS}
      items={plan}
      keys={keys}
      measureKey={bulkKey}
      ref={list}
      render={(n) => (
        <ChatNodeView
          a={a}
          activeMark={activeMark}
          allOpen={allOpen}
          isOpen={isOpen}
          marks={marks}
          n={n}
          onToggle={onToggle}
          queryActive={queryActive}
          tools={tools}
        />
      )}
      scrollPaddingStart={stickyHeight}
      testId="chat-transcript"
    />
  );
};
