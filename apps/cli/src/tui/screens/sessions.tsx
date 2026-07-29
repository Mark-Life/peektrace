/** Sessions screen — master-detail context-debug view.
 *
 * Left: a filterable, mouse- and keyboard-navigable rail of transcript sessions
 * from `sessions.list`, live-refreshed off the filesystem watch, rendered as
 * clickable cards. Right: the selected session's forensic analysis
 * (`SessionDetail`). Focus moves between the list and the history scroll with
 * Tab (or Enter/→ into detail, ←/Esc back); `/` opens the free-text filter, `a`
 * cycles the agent facet, `r` toggles secret redaction. Mirrors the web
 * inspector's sessions route, retuned for a narrow terminal.
 */
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { useEffect, useMemo, useState } from "react";
import {
  Card,
  Empty,
  ErrorLine,
  KeyHints,
  Loading,
  Panel,
} from "../components";
import { useQuery, useWatch } from "../runtime";
import {
  AGENT_COLOR,
  AGENT_LABEL,
  C,
  clip,
  fmtBytes,
  fmtStarted,
  sanitize,
} from "../theme";
import { useTyping } from "../typing";
import { useListSelection, windowSlice } from "../use-list";
import { SessionDetail } from "./sessions-detail";

/** Fixed width (cells) of the left session-card rail. */
const LIST_W = 46;
/** Content width inside a card (rail minus border + horizontal padding). */
const CARD_CONTENT_W = LIST_W - 4;
/** Chars reserved for the leading agent badge on a card's title line. */
const BADGE_W = 12;
/** Terminal rows one card spans: 2 border + 2 content + 1 inter-card gap. */
const CARD_ROWS = 5;
/** Non-card terminal rows (header, filter, agent line, footer) to reserve. */
const CHROME_ROWS = 11;
/** Minimum visible cards regardless of terminal height. */
const MIN_CARDS = 2;

/** Display title for a header, falling back to an agent-labelled generic. */
const titleOf = (h: SessionHeader): string =>
  h.title ?? `${AGENT_LABEL[h.agent] ?? h.agent} session`;

/** Sort comparator: `startedAt` descending, empty last. */
const byStartedDesc = (a: SessionHeader, b: SessionHeader): number => {
  const sa = a.startedAt ?? "";
  const sb = b.startedAt ?? "";
  if (sa === sb) {
    return 0;
  }
  if (sa === "") {
    return 1;
  }
  if (sb === "") {
    return -1;
  }
  return sb.localeCompare(sa);
};

/**
 * One session card: a clickable bordered box with an agent badge + title line
 * and a dim metadata subline. Clicking selects it via `onSelect`.
 */
const SessionCard = ({
  h,
  selected,
  onSelect,
}: {
  readonly h: SessionHeader;
  readonly selected: boolean;
  readonly onSelect: () => void;
}) => {
  const model = h.model ?? h.id.slice(0, 8);
  const sub = `${model} · ${fmtStarted(h.startedAt)} · ${fmtBytes(
    h.sizeBytes
  )} · ${h.messageCount} msgs`;
  return (
    <Card onSelect={onSelect} selected={selected}>
      <box style={{ flexDirection: "row" }}>
        <text fg={AGENT_COLOR[h.agent] ?? C.textDim}>
          {`[${AGENT_LABEL[h.agent] ?? h.agent}] `}
        </text>
        <text fg={selected ? C.primary : C.text}>
          {clip(sanitize(titleOf(h)), CARD_CONTENT_W - BADGE_W)}
        </text>
      </box>
      <text fg={C.textFaint}>{clip(sub, CARD_CONTENT_W)}</text>
    </Card>
  );
};

/** The Sessions screen. */
export const SessionsScreen = () => {
  const { sessions: sessionsVer } = useWatch();
  const q = useQuery((c) => c.sessions.list({}), [sessionsVer]);
  const headers = q.data ?? [];

  const [focus, setFocus] = useState<"list" | "detail">("list");
  const [query, setQuery] = useState("");
  const [filterFocused, setFilterFocused] = useState(false);
  const [agent, setAgent] = useState<SessionHeader["agent"] | undefined>(
    undefined
  );
  const [redact, setRedact] = useState(true);
  const { setTyping } = useTyping();

  useEffect(() => {
    setTyping(filterFocused);
  }, [filterFocused, setTyping]);

  const agents = useMemo(
    () => [...new Set(headers.map((h) => h.agent))].sort(),
    [headers]
  );

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return headers
      .filter((h) => agent === undefined || h.agent === agent)
      .filter(
        (h) =>
          needle === "" ||
          `${titleOf(h)} ${h.id}`.toLowerCase().includes(needle)
      )
      .slice()
      .sort(byStartedDesc);
  }, [headers, query, agent]);

  const listActive = focus === "list" && !filterFocused;
  const [index, setIndex] = useListSelection(filtered.length, listActive);
  const selected = filtered[index];

  const { height } = useTerminalDimensions();
  const visible = Math.max(
    MIN_CARDS,
    Math.floor((height - CHROME_ROWS) / CARD_ROWS)
  );
  const win = windowSlice(index, filtered.length, visible);

  useKeyboard((key) => {
    // Tab toggles focus in either direction, ungated (handled only here).
    if (key.name === "tab") {
      setFocus((f) => (f === "list" ? "detail" : "list"));
      return;
    }
    if (filterFocused) {
      if (key.name === "escape") {
        setFilterFocused(false);
      }
      return;
    }
    if (key.name === "/" || key.sequence === "/") {
      setFilterFocused(true);
      return;
    }
    if (!listActive) {
      return;
    }
    switch (key.name) {
      case "return":
      case "right":
        setFocus("detail");
        break;
      case "r":
        setRedact((r) => !r);
        break;
      case "a": {
        if (agents.length > 0) {
          const cycle = [undefined, ...agents];
          const at = cycle.indexOf(agent);
          setAgent(cycle[(at + 1) % cycle.length]);
          setIndex(0);
        }
        break;
      }
      default:
        break;
    }
  });

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 0 }}>
      <box style={{ flexDirection: "row", flexGrow: 1, minHeight: 0, gap: 1 }}>
        <Panel
          focused={focus === "list"}
          title={`Sessions (${filtered.length}/${headers.length})`}
          width={LIST_W}
        >
          <box style={{ flexDirection: "row" }}>
            <text fg={filterFocused ? C.primary : C.textFaint}>/ </text>
            <input
              focused={filterFocused}
              onInput={setQuery}
              onSubmit={() => setFilterFocused(false)}
              placeholder="filter title / id…"
              style={{ flexGrow: 1 }}
            />
          </box>
          <text fg={C.textFaint}>
            {agent === undefined
              ? "agent: all · a to cycle"
              : `agent: ${AGENT_LABEL[agent] ?? agent} · a to cycle`}
          </text>
          {q.loading && headers.length === 0 ? <Loading /> : null}
          {q.error ? <ErrorLine error={q.error} /> : null}
          {!(q.loading || q.error) && headers.length === 0 ? (
            <Empty label="No sessions found." />
          ) : null}
          {headers.length > 0 && filtered.length === 0 ? (
            <Empty label="No sessions match the filter." />
          ) : null}
          <box style={{ flexDirection: "column", gap: 1, paddingTop: 1 }}>
            {filtered.slice(win.start, win.end).map((h, i) => {
              const abs = win.start + i;
              return (
                <SessionCard
                  h={h}
                  key={h.id}
                  onSelect={() => setIndex(abs)}
                  selected={abs === index}
                />
              );
            })}
          </box>
        </Panel>
        {selected ? (
          <SessionDetail
            focused={focus === "detail"}
            id={selected.id}
            onBack={() => setFocus("list")}
            redact={redact}
          />
        ) : (
          <Panel flexGrow={1} title="Analysis">
            <Empty label="Select a session to analyze." />
          </Panel>
        )}
      </box>
      <KeyHints
        hints={[
          ["↑↓/jk", "move"],
          ["enter/→", "open"],
          ["tab", "switch"],
          ["/", "filter"],
          ["a", "agent"],
          ["r", redact ? "reveal" : "hide"],
          ["click", "select"],
          ["q", "quit"],
        ]}
      />
    </box>
  );
};
