/** Scrollable, expandable, syntax-highlighted session history.
 *
 * Renders the analyzed session's timeline (minus `system` events) as navigable
 * rows: each header shows its turn tag, a kind/tool badge, a one-line preview and
 * a token estimate; expanding a row reveals the full body decoded per
 * `history-decode` and highlighted per language. Active only when `focused`;
 * Left/Esc calls `onBack` to hand focus back to the session list. Tab is owned by
 * the parent screen and deliberately not handled here.
 */
import type { ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
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
  onSelect,
}: {
  readonly e: TimelineEvent;
  readonly pos: number;
  readonly turn: number;
  readonly selected: boolean;
  readonly open: boolean;
  readonly previewMax: number;
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
        label={clip(e.toolName ?? e.kind, BADGE_MAX)}
      />
      <text fg={selected ? C.primary : C.text}>
        {` ${firstLine(e.preview, previewMax)}`}
      </text>
      <box style={{ flexGrow: 1 }} />
      <text fg={C.textFaint}>{` ~${fmt(e.tokensEst)}`}</text>
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
  const visible = useMemo(() => visibleEvents(s), [s]);
  const tags = useMemo(() => turnTags(s), [s]);
  const { width } = useTerminalDimensions();
  const previewMax = Math.max(MIN_PREVIEW_W, width - PANE_CHROME - ROW_FIXED);
  const [index, setIndex] = useListSelection(visible.length, focused);
  const [openSet, setOpenSet] = useState<Set<number>>(() => new Set());
  const [expandAll, setExpandAll] = useState(false);
  const boxRef = useRef<ScrollBoxRenderable>(null);

  const collapseAll = useCallback(() => {
    setExpandAll(false);
    setOpenSet(new Set());
  }, []);

  const toggle = useCallback(
    (pos: number) => {
      if (expandAll) {
        // Materialize every other row as open, drop the flag, close this one.
        const rest = new Set<number>();
        for (let i = 0; i < visible.length; i++) {
          if (i !== pos) {
            rest.add(i);
          }
        }
        setOpenSet(rest);
        setExpandAll(false);
        return;
      }
      setOpenSet((prev) => {
        const next = new Set(prev);
        if (next.has(pos)) {
          next.delete(pos);
        } else {
          next.add(pos);
        }
        return next;
      });
    },
    [expandAll, visible.length]
  );

  useKeyboard((key) => {
    if (!focused) {
      return;
    }
    if (key.name === "left" || key.name === "escape") {
      onBack();
    } else if (key.name === "return") {
      toggle(index);
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
          <text fg={C.textFaint}> enter toggle · shift+E/C all</text>
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
                  toggle(pos);
                }}
                open={expandAll || openSet.has(pos)}
                pos={pos}
                previewMax={previewMax}
                selected={pos === index}
                turn={tags.get(e.index) ?? 0}
              />
            ))}
          </box>
        </scrollbox>
      )}
    </box>
  );
};
