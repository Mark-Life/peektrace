/** Capabilities screen — the feature-support matrix.
 *
 * Left: a navigable list of capabilities (grouped) with one colored support dot
 * per agent column. Right: the selected capability's description and each
 * agent's level + plain-language gloss. A legend maps colors to levels. Mirrors
 * the web inspector's matrix, retuned for a narrow terminal.
 */
import { useTerminalDimensions } from "@opentui/react";
import type { Capability } from "@workspace/rpc";
import {
  Empty,
  ErrorLine,
  KeyHints,
  Loading,
  Panel,
  Swatch,
} from "../components";
import { useQuery } from "../runtime";
import { C, clip, LEVEL_COLOR, LEVEL_GLOSS } from "../theme";
import { useListSelection, windowSlice } from "../use-list";

const AGENT_COLUMNS = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "pi", label: "Pi" },
  { id: "opencode", label: "OpenCode" },
] as const;

const LEVELS = ["supported", "partial", "planned", "unsupported"] as const;

const NAME_W = 30;
const COL_W = 9;
const DETAIL_W = 42;
/** Chars of description shown in the detail pane (two wrapped rows). */
const DESC_MAX = (DETAIL_W - 4) * 2;
/** Chars of a per-agent note shown under each agent in the detail pane. */
const NOTE_MAX = DETAIL_W - 6;
/** Non-list terminal rows (header, footer, borders, legend) to reserve. */
const CHROME_ROWS = 10;
/** Minimum visible list rows regardless of terminal height. */
const MIN_ROWS = 4;

/** Support level for `cap` under `agentId`, tolerant of a missing cell. */
const levelOf = (cap: Capability, agentId: string) =>
  cap.perAgent[agentId as keyof typeof cap.perAgent]?.level ?? "unsupported";

/** Header row: the `Capability` label + the four agent column labels. */
const MatrixHeader = () => (
  <box style={{ flexDirection: "row" }}>
    <text fg={C.textDim}>{"Capability".padEnd(NAME_W)}</text>
    {AGENT_COLUMNS.map((a) => (
      <box key={a.id} style={{ width: COL_W }}>
        <text fg={C.textDim}>{a.label}</text>
      </box>
    ))}
  </box>
);

/** One capability row: title + a colored dot per agent. */
const MatrixRow = ({
  cap,
  selected,
}: {
  readonly cap: Capability;
  readonly selected: boolean;
}) => (
  <box
    style={{
      flexDirection: "row",
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
  >
    <text fg={selected ? C.primary : C.text}>
      {(selected ? "› " : "  ") + clip(cap.title, NAME_W - 2)}
    </text>
    <text>
      {"".padEnd(Math.max(0, NAME_W - 2 - clip(cap.title, NAME_W - 2).length))}
    </text>
    {AGENT_COLUMNS.map((a) => (
      <box key={a.id} style={{ width: COL_W }}>
        <text fg={LEVEL_COLOR[levelOf(cap, a.id)]}>●</text>
      </box>
    ))}
  </box>
);

/** Right pane: the selected capability's per-agent detail. */
const Detail = ({ cap }: { readonly cap: Capability }) => (
  <Panel title="Detail" width={DETAIL_W}>
    <text fg={C.text}>{cap.title}</text>
    <text fg={C.textDim}>{clip(cap.description, DESC_MAX)}</text>
    <text> </text>
    {AGENT_COLUMNS.map((a) => {
      const cell = cap.perAgent[a.id as keyof typeof cap.perAgent];
      const level = cell?.level ?? "unsupported";
      return (
        <box key={a.id} style={{ flexDirection: "column" }}>
          <box style={{ flexDirection: "row" }}>
            <text fg={C.textDim}>{a.label.padEnd(10)}</text>
            <text fg={LEVEL_COLOR[level]}>{`● ${level}`}</text>
          </box>
          <text
            fg={C.textFaint}
          >{`  ${clip(cell?.note ?? LEVEL_GLOSS[level], NOTE_MAX)}`}</text>
        </box>
      );
    })}
  </Panel>
);

/** Bottom legend: color swatch + level name for each support level. */
const Legend = () => (
  <box style={{ flexDirection: "row", gap: 2, paddingTop: 0 }}>
    {LEVELS.map((level) => (
      <box key={level} style={{ flexDirection: "row" }}>
        <Swatch color={LEVEL_COLOR[level]} />
        <text fg={C.textFaint}>{` ${level}`}</text>
      </box>
    ))}
  </box>
);

/** The Capabilities screen. */
export const CapabilitiesScreen = () => {
  const caps = useQuery((c) => c.capabilities.list(), []);
  const list = caps.data ?? [];
  const [index] = useListSelection(list.length, true);
  const { height } = useTerminalDimensions();
  const visible = Math.max(MIN_ROWS, height - CHROME_ROWS);
  const win = windowSlice(index, list.length, visible);
  const selected = list[index];

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 0 }}>
      <box style={{ flexDirection: "row", flexGrow: 1, minHeight: 0, gap: 1 }}>
        <Panel flexGrow={1} focused title={`Capabilities (${list.length})`}>
          {caps.loading ? <Loading /> : null}
          {caps.error ? <ErrorLine error={caps.error} /> : null}
          {!(caps.loading || caps.error) && list.length === 0 ? (
            <Empty label="No capabilities reported." />
          ) : null}
          {list.length > 0 ? (
            <box style={{ flexDirection: "column" }}>
              <MatrixHeader />
              {list.slice(win.start, win.end).map((cap, i) => {
                const abs = win.start + i;
                const prev = list[abs - 1];
                const groupHeader =
                  prev === undefined || prev.group !== cap.group ? (
                    <text fg={C.accent}>{cap.group.toUpperCase()}</text>
                  ) : null;
                return (
                  <box key={cap.id} style={{ flexDirection: "column" }}>
                    {groupHeader}
                    <MatrixRow cap={cap} selected={abs === index} />
                  </box>
                );
              })}
            </box>
          ) : null}
        </Panel>
        {selected ? <Detail cap={selected} /> : null}
      </box>
      <Legend />
      <KeyHints
        hints={[
          ["↑↓/jk", "move"],
          ["1-4", "section"],
          ["q", "quit"],
        ]}
      />
    </box>
  );
};
