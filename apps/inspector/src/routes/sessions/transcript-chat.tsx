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
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { tokenLabel } from "@workspace/core/services/sessions/tokens";
import type { SessionMarker } from "@workspace/core/services/stats/schema";
import { Markdown } from "@workspace/ui/components/ai-elements/markdown";
import { cn } from "@workspace/ui/lib/utils";
import { fmt, fmtK, PERCENT } from "@workspace/viz/lib/session-format";
import { FileTextIcon, ScissorsIcon, SettingsIcon } from "lucide-react";
import { memo, type RefObject, useCallback, useMemo, useRef } from "react";
import type { GrowthTurn } from "../../lib/api-types.gen";
import {
  buildChatPlan,
  type ChatNode,
  chatCollapseIdOf,
  chatNodeKeys,
  type EventRow,
  laneOf,
  type TranscriptRowRef,
} from "../../lib/chat-lane";
import {
  alignOf,
  type HistoryJump,
  type JumpLanding,
  planIndexOfTarget,
} from "../../lib/history-jump";
import {
  type HydrateCache,
  hydrator,
  positionsOf,
} from "../../lib/history-pages";
import type { ToolMarks } from "../../lib/session-markers";
import {
  chatCollapseId,
  chatPanelId,
  disclosureProps,
} from "../../lib/session-view";
import type { ToolIndex } from "../../lib/tool-event";
import { toolNameOf, toolState } from "../../lib/tool-event";
import {
  CHAT_GAP,
  CHAT_ITEM_CLASS,
  estimateChatNode,
} from "../../lib/transcript-estimate";
import { chatTurnStops } from "../../lib/turn-nav";
import { type RevealHit, useRevealHit } from "../../lib/use-history-search";
import { useTurnNav } from "../../lib/use-turn-nav";
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
import { PendingRow } from "./pending-row";
import { VirtualList } from "./virtual-list";

/** What the chat reads besides the rows: the dumb zone, and per-turn usage
 *  for the turn rules (`turns[n - 1]` is turn `n`). */
export type ChatSession = Pick<
  AnalyzedSession,
  "contextWindow" | "dumbZoneCrossTurn" | "dumbZoneFraction"
> & { readonly turns: readonly GrowthTurn[] };

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
          {...disclosureProps(chatPanelId(id), open)}
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
          <div
            className="mt-1 flex flex-col gap-1 text-left"
            id={chatPanelId(id)}
          >
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
  readonly a: ChatSession;
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
 *  mounted; `jump` is the place a link or the summary asked to land on. */
export const TranscriptChat = ({
  a,
  activeMark,
  allOpen,
  bulkKey,
  crossEvtIdx,
  eventAt,
  isOpen,
  jump,
  landing,
  marks,
  onDrawn,
  onToggle,
  queryActive,
  revealHit,
  rows,
  stickyHeight,
  tools,
  turns,
}: {
  readonly a: ChatSession;
  /** The detector id the reader arrived with, highlighted where it applies. */
  readonly activeMark: string | null;
  readonly allOpen: boolean;
  /** Bumped by expand or collapse all, when every height changes at once. */
  readonly bulkKey: number;
  readonly crossEvtIdx: number;
  /** The whole event at a position, when rows are index rows; null while
   *  its page loads. */
  readonly eventAt?: (pos: number) => TimelineEvent | null;
  readonly isOpen: (id: string) => boolean;
  /** Where a link or the summary asked to go, if anywhere. */
  readonly jump: HistoryJump | null;
  /** How far the jump got, shared by both layouts. */
  readonly landing: JumpLanding;
  /** Stats marks for this session, keyed by tool-use id. */
  readonly marks: ToolMarks;
  /** Event positions of the drawn blocks, after each change. */
  readonly onDrawn?: (positions: readonly number[]) => void;
  readonly onToggle: (id: string, open: boolean) => void;
  /** A search is running: chip clusters must not fold a match out of sight. */
  readonly queryActive: boolean;
  /** Filled here so search can step to a hit that is not drawn. */
  readonly revealHit: RefObject<RevealHit | null>;
  readonly rows: readonly TranscriptRowRef<EventRow>[];
  /** Height of the sticky toolbar a jump must land below. */
  readonly stickyHeight: number;
  readonly tools: ToolIndex;
  readonly turns: readonly number[];
}) => {
  const cache = useRef<HydrateCache>(new WeakMap());
  const hydrate = useMemo(() => hydrator(eventAt, cache.current), [eventAt]);
  const plan = useMemo(
    () => buildChatPlan({ crossEvtIdx, rows, turns }),
    [crossEvtIdx, rows, turns]
  );
  const keys = useMemo(() => chatNodeKeys(plan), [plan]);
  const list = useRef<VirtualListHandle>(null);
  useVirtualJump({
    align: jump ? alignOf(jump.target) : "start",
    done: landing.done,
    index: jump ? planIndexOfTarget(plan, jump.target) : -1,
    list,
    target: jump?.key ?? null,
  });
  const indexOfHit = useCallback(
    (id: string) =>
      plan.findIndex(
        (n) =>
          chatCollapseIdOf(n) === id ||
          (n.type === "run" &&
            n.items.some(({ pos }) => chatCollapseId(pos) === id))
      ),
    [plan]
  );
  useRevealHit({ indexOf: indexOfHit, list, reveal: revealHit });
  const stops = useMemo(() => chatTurnStops(plan), [plan]);
  useTurnNav({ list, stops });
  const onRange = useCallback(
    (first: number, last: number) =>
      onDrawn?.(plan.slice(first, last + 1).flatMap(positionsOf)),
    [plan, onDrawn]
  );
  const estimate = (n: ChatNode<EventRow>) =>
    estimateChatNode({ allOpen, isOpen, n });

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
      estimate={estimate}
      itemClassName={CHAT_ITEM_CLASS}
      items={plan}
      keys={keys}
      measureKey={bulkKey}
      onRange={onRange}
      ref={list}
      render={(n) => {
        const full = hydrate.node(n);
        return full === null ? (
          <PendingRow height={estimate(n) - CHAT_GAP} />
        ) : (
          <ChatNodeView
            a={a}
            activeMark={activeMark}
            allOpen={allOpen}
            isOpen={isOpen}
            marks={marks}
            n={full}
            onToggle={onToggle}
            queryActive={queryActive}
            tools={tools}
          />
        );
      }}
      scrollPaddingStart={stickyHeight}
      testId="chat-transcript"
    />
  );
};
