/** Sessions screen — master-detail context-debug view.
 *
 * Left: a filterable, keyboard-navigable list of transcript sessions from
 * `sessions.list`, live-refreshed off the filesystem watch. Right: the selected
 * session's forensic analysis (`SessionDetail`). Focus moves between the list
 * and the history scroll with Tab; `/` opens the free-text filter, `a` cycles
 * the agent facet, `r` toggles secret redaction. Mirrors the web inspector's
 * sessions route, retuned for a narrow terminal.
 */
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { useEffect, useMemo, useState } from "react";
import { Empty, ErrorLine, KeyHints, Loading, Panel } from "../components";
import { useQuery, useWatch } from "../runtime";
import {
  AGENT_COLOR,
  AGENT_LABEL,
  C,
  clip,
  fmtBytes,
  fmtStarted,
} from "../theme";
import { useTyping } from "../typing";
import { useListSelection, windowSlice } from "../use-list";
import { SessionDetail } from "./sessions-detail";

/** Fixed width (cells) of the left session-list rail. */
const LIST_W = 42;
/** Terminal rows reserved for chrome (header, filter, borders, hints). */
const CHROME_ROWS = 9;
/** Minimum visible session rows regardless of terminal height. */
const MIN_ROWS = 3;
/** Terminal lines each session row occupies (title + footer). */
const ROW_LINES = 2;

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

/** One two-line session row: agent badge + title, then a metadata footer. */
const SessionRow = ({
  h,
  selected,
}: {
  readonly h: SessionHeader;
  readonly selected: boolean;
}) => {
  const model = h.model ?? h.id.slice(0, 8);
  const footer = `${model} · ${fmtStarted(h.startedAt)} · ${fmtBytes(
    h.sizeBytes
  )} · ${h.messageCount} msgs`;
  return (
    <box
      style={{
        flexDirection: "column",
        ...(selected ? { backgroundColor: C.panelSel } : {}),
      }}
    >
      <box style={{ flexDirection: "row" }}>
        <text fg={AGENT_COLOR[h.agent] ?? C.textDim}>
          {`${selected ? "› " : "  "}[${AGENT_LABEL[h.agent] ?? h.agent}] `}
        </text>
        <text fg={selected ? C.primary : C.text}>
          {clip(titleOf(h), LIST_W - 12)}
        </text>
      </box>
      <text fg={C.textFaint}>{`  ${clip(footer, LIST_W - 4)}`}</text>
    </box>
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
    MIN_ROWS,
    Math.floor((height - CHROME_ROWS) / ROW_LINES)
  );
  const win = windowSlice(index, filtered.length, visible);

  useKeyboard((key) => {
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
    switch (key.name) {
      case "tab":
        setFocus((f) => (f === "list" ? "detail" : "list"));
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
          <box style={{ flexDirection: "column" }}>
            {filtered.slice(win.start, win.end).map((h, i) => (
              <SessionRow h={h} key={h.id} selected={win.start + i === index} />
            ))}
          </box>
        </Panel>
        {selected ? (
          <SessionDetail
            focused={focus === "detail"}
            id={selected.id}
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
          ["tab", focus === "list" ? "history" : "list"],
          ["/", "filter"],
          ["a", "agent"],
          ["r", redact ? "reveal" : "hide"],
          ["1-4", "section"],
          ["q", "quit"],
        ]}
      />
    </box>
  );
};
