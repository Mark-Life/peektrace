/** Scrollable, expandable, syntax-highlighted session history.
 *
 * Renders the analyzed session's timeline (minus `system` events) as navigable
 * rows: each header shows its turn tag, a kind/tool badge, a one-line preview and
 * a token estimate; expanding a row reveals the full body decoded per
 * `history-decode` and highlighted per language. `s` re-sorts the rows biggest
 * first, which is how the biggest context items are found. Active only when `focused`;
 * Left/Esc calls `onBack` to hand focus back to the session list. Tab is owned by
 * the parent screen and deliberately not handled here.
 */
import type { ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import { eventBadgeLabel } from "@workspace/core/services/sessions/labels";
import type {
  AnalyzedSession,
  EventKind,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge, Empty, TextButton } from "../components";
import { Highlighted } from "../syntax";
import { C, clip, firstLine, fmt } from "../theme";
import { useListSelection } from "../use-list";
import {
  type DecodedBody,
  decodeBody,
  turnTags,
  visibleEvents,
} from "./history-decode";

/** Badge tint per event kind (errors override to `bad`). */
const KIND_COLOR: Record<EventKind, string> = {
  "user-prompt": C.primary,
  "assistant-text": C.text,
  "assistant-thinking": C.accent,
  "tool-call": C.info,
  "tool-result": C.good,
  attachment: C.warn,
  system: C.textFaint,
  compaction: C.warn,
  summary: C.accent,
  meta: C.textFaint,
};

/** Row order: transcript order, or largest events first. */
type SortMode = "order" | "size";

/** Chars of the kind/tool badge label. */
const BADGE_MAX = 20;
/** Line budget for an expanded body before elision. */
const BODY_MAX_LINES = 120;
/** Cells the left rail + gaps + this pane's chrome claim, for preview sizing. */
const PANE_CHROME = 52;
/** Cells a history row spends on badges + the token estimate (before preview). */
const ROW_FIXED = 30;
/** Floor on the body preview width, so a tiny terminal still shows something. */
const MIN_PREVIEW_W = 16;
/** Extra cells the window-share column claims in size order. */
const SHARE_W = 7;
/** Percent scale for the window share. */
const PERCENT = 100;
/** Decimal places for the per-event window share. */
const SHARE_DECIMALS = 1;

/** Expanded body: the dim thinking note when present, else the highlighted code. */
const ItemBody = ({ decoded }: { readonly decoded: DecodedBody }) => {
  if (decoded.note !== undefined) {
    return (
      <box style={{ paddingLeft: 2 }}>
        <text fg={C.textFaint}>{decoded.note}</text>
      </box>
    );
  }
  return (
    <box style={{ paddingLeft: 2 }}>
      <Highlighted
        content={decoded.content}
        lang={decoded.lang}
        maxLines={BODY_MAX_LINES}
      />
    </box>
  );
};

/** One timeline row: clickable header + (when open) the decoded body below it. */
const HistoryItem = ({
  e,
  pos,
  turn,
  selected,
  open,
  previewMax,
  share,
  onSelect,
}: {
  readonly e: TimelineEvent;
  readonly pos: number;
  readonly turn: number;
  readonly selected: boolean;
  readonly open: boolean;
  readonly previewMax: number;
  /** Share of the context window, shown only in size order. */
  readonly share?: number | undefined;
  readonly onSelect: () => void;
}) => (
  <box id={`hist:${pos}`} style={{ flexDirection: "column" }}>
    {/** biome-ignore lint/a11y/noStaticElementInteractions: OpenTUI box is the only click target; no DOM roles apply */}
    <box
      onMouseDown={onSelect}
      style={{
        flexDirection: "row",
        ...(selected ? { backgroundColor: C.panelSel } : {}),
      }}
    >
      <Badge label={`t${turn}`} />
      <text> </text>
      <Badge
        color={e.isError ? C.bad : KIND_COLOR[e.kind]}
        label={clip(eventBadgeLabel(e), BADGE_MAX)}
      />
      <text fg={selected ? C.primary : C.text}>
        {` ${firstLine(e.preview, previewMax)}`}
      </text>
      <box style={{ flexGrow: 1 }} />
      <text fg={C.textFaint}>{` ~${fmt(e.tokensEst)}`}</text>
      {share === undefined ? null : (
        <text fg={C.textFaint}>
          {` ${(share * PERCENT).toFixed(SHARE_DECIMALS)}%`.padStart(SHARE_W)}
        </text>
      )}
    </box>
    {open ? <ItemBody decoded={decodeBody(e)} /> : null}
  </box>
);

/** The full session history view. */
export const SessionHistory = ({
  s,
  redact,
  focused,
  onBack,
}: {
  readonly s: AnalyzedSession;
  readonly redact: boolean;
  readonly focused: boolean;
  readonly onBack: () => void;
}) => {
  const [sort, setSort] = useState<SortMode>("order");
  const events = useMemo(() => visibleEvents(s), [s]);
  const visible = useMemo(
    () =>
      sort === "size"
        ? [...events].sort((a, b) => b.tokensEst - a.tokensEst)
        : events,
    [events, sort]
  );
  const tags = useMemo(() => turnTags(s), [s]);
  const { width } = useTerminalDimensions();
  const previewMax = Math.max(
    MIN_PREVIEW_W,
    width - PANE_CHROME - ROW_FIXED - (sort === "size" ? SHARE_W : 0)
  );
  const [index, setIndex] = useListSelection(visible.length, focused);
  // Open rows are keyed by the event's own index, so re-sorting keeps the same
  // rows open instead of the same screen positions.
  const [openSet, setOpenSet] = useState<Set<number>>(() => new Set());
  const [expandAll, setExpandAll] = useState(false);
  const boxRef = useRef<ScrollBoxRenderable>(null);

  const collapseAll = useCallback(() => {
    setExpandAll(false);
    setOpenSet(new Set());
  }, []);

  const toggle = useCallback(
    (eventIndex: number) => {
      if (expandAll) {
        // Materialize every other row as open, drop the flag, close this one.
        const rest = new Set(
          visible.map((e) => e.index).filter((i) => i !== eventIndex)
        );
        setOpenSet(rest);
        setExpandAll(false);
        return;
      }
      setOpenSet((prev) => {
        const next = new Set(prev);
        if (next.has(eventIndex)) {
          next.delete(eventIndex);
        } else {
          next.add(eventIndex);
        }
        return next;
      });
    },
    [expandAll, visible]
  );

  useKeyboard((key) => {
    if (!focused) {
      return;
    }
    if (key.name === "left" || key.name === "escape") {
      onBack();
    } else if (key.name === "return") {
      const e = visible[index];
      if (e) {
        toggle(e.index);
      }
    } else if (key.name === "s" || key.sequence === "s") {
      setSort((m) => (m === "order" ? "size" : "order"));
      setIndex(0);
    } else if (key.name === "e" && key.shift) {
      setExpandAll(true);
    } else if (key.name === "c" && key.shift) {
      collapseAll();
    }
  });

  useEffect(() => {
    boxRef.current?.scrollChildIntoView(`hist:${index}`);
  }, [index]);

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 0 }}>
      <box style={{ flexDirection: "column", flexShrink: 0 }}>
        {/* Header line: title + counts + redaction state. */}
        <box style={{ flexDirection: "row", gap: 2 }}>
          <text fg={C.accent}>Full history</text>
          <text fg={C.textDim}>
            {`${visible.length} events · ${s.dumbZoneTurns}/${s.turnCount} in dumb zone`}
          </text>
          <box style={{ flexGrow: 1 }} />
          {redact ? (
            <text fg={C.textFaint}>redacted · r to reveal</text>
          ) : (
            <text fg={C.bad}>⚠ Redaction OFF</text>
          )}
        </box>
        {/* Toolbar on its own line so it never collides with the first row. */}
        <box style={{ flexDirection: "row", gap: 1 }}>
          <TextButton
            active={expandAll}
            label="Expand all"
            onPress={() => setExpandAll(true)}
          />
          <TextButton
            active={!expandAll && openSet.size === 0}
            label="Collapse all"
            onPress={collapseAll}
          />
          <TextButton
            active={sort === "size"}
            label="Biggest first"
            onPress={() => {
              setSort((m) => (m === "order" ? "size" : "order"));
              setIndex(0);
            }}
          />
          <text fg={C.textFaint}> enter toggle · s sort · shift+E/C all</text>
        </box>
      </box>
      {visible.length === 0 ? (
        <Empty label="No timeline events." />
      ) : (
        <scrollbox
          focused={focused}
          ref={boxRef}
          style={{ flexGrow: 1, minHeight: 0, paddingTop: 1 }}
        >
          <box style={{ flexDirection: "column", gap: 1 }}>
            {visible.map((e, pos) => (
              <HistoryItem
                e={e}
                key={`hist:${e.index}`}
                onSelect={() => {
                  setIndex(pos);
                  toggle(e.index);
                }}
                open={expandAll || openSet.has(e.index)}
                pos={pos}
                previewMax={previewMax}
                selected={pos === index}
                share={
                  sort === "size" && s.contextWindow > 0
                    ? e.tokensEst / s.contextWindow
                    : undefined
                }
                turn={tags.get(e.index) ?? 0}
              />
            ))}
          </box>
        </scrollbox>
      )}
    </box>
  );
};
