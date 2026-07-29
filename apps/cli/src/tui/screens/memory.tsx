/** Memory screen — a cross-project browser over every Claude vault.
 *
 * Left: a navigable flat list, entries grouped under a dim per-vault header
 * (project + matched count + an "over budget" tag). Right: the selected entry's
 * detail with a scrollable body preview, or — when a group header is selected —
 * the vault's budget gauge, type counts, and index/files diff. Read-only: the
 * write surface (create/edit/delete) lives in the web app; here we only gate on
 * the `memory.crud` capability to surface that. Mirrors the web inspector,
 * retuned for a narrow terminal.
 */
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import type {
  MemoryEntry,
  MemoryVault,
} from "@workspace/core/services/memory/types";
import { useState } from "react";
import {
  Badge,
  Empty,
  ErrorLine,
  Field,
  KeyHints,
  Loading,
  Panel,
  StackedBar,
} from "../components";
import { useQuery, useWatch } from "../runtime";
import { C, clip, fmtBytes, fmtStarted } from "../theme";
import { useTyping } from "../typing";
import { useListSelection, windowSlice } from "../use-list";

/** Type-filter cycle: `all`, each documented type, then untyped. */
const TYPE_FILTERS = [
  "all",
  "user",
  "feedback",
  "project",
  "reference",
  "untyped",
] as const;
type TypeFilter = (typeof TYPE_FILTERS)[number];

/** The next type filter in the cycle, wrapping past the last. */
const nextTypeFilter = (current: TypeFilter): TypeFilter =>
  TYPE_FILTERS[(TYPE_FILTERS.indexOf(current) + 1) % TYPE_FILTERS.length] ??
  "all";

/** Callbacks the memory keymap invokes; kept flat so the hook stays simple. */
interface MemoryKeys {
  readonly onCloseSearch: () => void;
  readonly onCycleType: () => void;
  readonly onOpenSearch: () => void;
  readonly onToggleBody: () => void;
  readonly searching: boolean;
  readonly selectedIsEntry: boolean;
}

/** List keymap: `/` search, `t` type cycle, `Tab` body focus, `Esc` close. */
const useMemoryKeys = (keys: MemoryKeys) => {
  useKeyboard((key) => {
    if (key.name === "escape") {
      if (keys.searching) {
        keys.onCloseSearch();
      }
      return;
    }
    if (keys.searching) {
      return;
    }
    if (key.name === "/") {
      keys.onOpenSearch();
    } else if (key.name === "t") {
      keys.onCycleType();
    } else if (key.name === "tab" && keys.selectedIsEntry) {
      keys.onToggleBody();
    }
  });
};

/** The write capability id looked up in the matrix; only Claude supports it. */
const MEMORY_CRUD = "memory.crud";
const WRITE_AGENT = "claude";

const DETAIL_W = 52;
/** Gauge track width inside the detail panel. */
const GAUGE_W = DETAIL_W - 8;
/** Chars of an entry title shown in a left-pane row. */
const TITLE_W = 26;
/** Chars of the body preview loaded into the scrollbox. */
const BODY_MAX = 6000;
/** Description chars shown in the detail pane. */
const DESC_MAX = (DETAIL_W - 4) * 3;
/** Links / diff rows listed before collapsing to a "+N more" tail. */
const LIST_CAP = 6;
/** Non-list terminal rows (header, control bar, footer, borders) reserved. */
const CHROME_ROWS = 11;
/** Minimum visible list rows regardless of terminal height. */
const MIN_ROWS = 4;

/** A flat-list row: a vault group header or one of its entries. */
type Row =
  | {
      readonly kind: "header";
      readonly key: string;
      readonly vault: MemoryVault;
      readonly matched: number;
    }
  | {
      readonly kind: "entry";
      readonly key: string;
      readonly vault: MemoryVault;
      readonly entry: MemoryEntry;
    };

/** True when `entry` passes the free-text query and the type filter. */
const entryMatches = (
  entry: MemoryEntry,
  query: string,
  type: TypeFilter
): boolean => {
  const typeOk = type === "all" || (entry.type ?? "untyped") === type;
  if (!typeOk) {
    return false;
  }
  if (query === "") {
    return true;
  }
  const hay =
    `${entry.slug} ${entry.name ?? ""} ${entry.description ?? ""} ${entry.body}`.toLowerCase();
  return hay.includes(query);
};

/**
 * Flatten vaults into header + entry rows, applying the active filters. With a
 * filter active, only vaults with ≥1 match appear; otherwise every vault shows.
 */
const buildRows = (
  vaults: readonly MemoryVault[],
  query: string,
  type: TypeFilter
): Row[] => {
  const rows: Row[] = [];
  const filtering = query !== "" || type !== "all";
  for (const vault of vaults) {
    const matched = vault.entries.filter((e) => entryMatches(e, query, type));
    if (filtering && matched.length === 0) {
      continue;
    }
    rows.push({
      kind: "header",
      key: `h:${vault.slug}`,
      vault,
      matched: matched.length,
    });
    for (const entry of matched) {
      rows.push({
        kind: "entry",
        key: `e:${vault.slug}:${entry.slug}`,
        vault,
        entry,
      });
    }
  }
  return rows;
};

/** A dim per-vault group header with matched count and budget flag. */
const HeaderRow = ({
  vault,
  matched,
  selected,
}: {
  readonly vault: MemoryVault;
  readonly matched: number;
  readonly selected: boolean;
}) => (
  <box
    style={{
      flexDirection: "row",
      gap: 1,
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
  >
    <text fg={selected ? C.primary : C.accent}>
      {(selected ? "▸ " : "  ") + clip(vault.project, TITLE_W)}
    </text>
    <text fg={C.textFaint}>{`(${matched})`}</text>
    {vault.budget.overBudget ? (
      <Badge color={C.bad} label="over budget" />
    ) : null}
  </box>
);

/** One entry row: title + type badge on the left, size/index/links on the right. */
const EntryRow = ({
  entry,
  selected,
}: {
  readonly entry: MemoryEntry;
  readonly selected: boolean;
}) => (
  <box
    style={{
      flexDirection: "row",
      justifyContent: "space-between",
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
  >
    <box style={{ flexDirection: "row", gap: 1 }}>
      <text fg={selected ? C.primary : C.text}>
        {(selected ? "  › " : "    ") + clip(entry.name ?? entry.slug, TITLE_W)}
      </text>
      {entry.type === undefined ? (
        <text fg={C.textFaint}>—</text>
      ) : (
        <Badge color={C.textDim} label={entry.type} />
      )}
    </box>
    <text fg={C.textDim}>
      {`${fmtBytes(entry.size)} ${entry.inIndex ? "✓" : "✗"} ↗${entry.links.length}`}
    </text>
  </box>
);

/** Right pane when a vault header is selected: budget, types, diff. */
const VaultSummary = ({ vault }: { readonly vault: MemoryVault }) => {
  const { budget, diff, typeCounts } = vault;
  const remaining = Math.max(0, budget.maxLines - budget.lines);
  const usedColor = budget.overBudget ? C.bad : C.good;
  const types = Object.entries(typeCounts);
  return (
    <Panel title="Vault" width={DETAIL_W}>
      <text fg={C.text}>{clip(vault.project, DETAIL_W - 4)}</text>
      <text fg={C.textDim}>{clip(vault.slug, DETAIL_W - 4)}</text>
      <text> </text>
      <Field
        label="Index budget"
        value={`${budget.lines}/${budget.maxLines} lines`}
        valueColor={budget.overBudget ? C.bad : C.text}
      />
      <StackedBar
        segments={[
          { color: usedColor, weight: budget.lines },
          { color: C.border, weight: remaining },
        ]}
        width={GAUGE_W}
      />
      <Field
        label="Bytes"
        value={`${fmtBytes(budget.bytes)} / ${fmtBytes(budget.maxBytes)}`}
      />
      <text> </text>
      <text fg={C.textDim}>Types</text>
      {types.length === 0 ? (
        <Empty label="  none" />
      ) : (
        <box style={{ flexDirection: "row", gap: 2 }}>
          {types.map(([type, count]) => (
            <text fg={C.text} key={type}>{`${type} ${count}`}</text>
          ))}
        </box>
      )}
      <text> </text>
      <text fg={C.textDim}>Index ↔ files</text>
      {diff.orphans.length === 0 && diff.dangling.length === 0 ? (
        <Empty label="  index and files are in sync" />
      ) : (
        <box style={{ flexDirection: "column" }}>
          <text fg={C.warn}>{`  orphans ${diff.orphans.length}`}</text>
          {diff.orphans.slice(0, LIST_CAP).map((file) => (
            <text
              fg={C.textFaint}
              key={`o:${file}`}
            >{`    ${clip(file, DETAIL_W - 8)}`}</text>
          ))}
          <text fg={C.bad}>{`  dangling ${diff.dangling.length}`}</text>
          {diff.dangling.slice(0, LIST_CAP).map((d) => (
            <text
              fg={C.textFaint}
              key={`d:${d.line}:${d.target}`}
            >{`    ${clip(d.target, DETAIL_W - 12)} :${d.line}`}</text>
          ))}
        </box>
      )}
    </Panel>
  );
};

/** Right pane when an entry is selected: metadata + scrollable body preview. */
const EntryDetail = ({
  entry,
  bodyFocused,
}: {
  readonly entry: MemoryEntry;
  readonly bodyFocused: boolean;
}) => (
  <Panel focused={bodyFocused} title="Entry" width={DETAIL_W}>
    <text fg={C.text}>{clip(entry.name ?? entry.slug, DETAIL_W - 4)}</text>
    <box style={{ flexDirection: "row", gap: 1 }}>
      {entry.type === undefined ? (
        <text fg={C.textFaint}>untyped</text>
      ) : (
        <Badge color={C.accent} label={entry.type} />
      )}
      <text fg={entry.inIndex ? C.good : C.bad}>
        {entry.inIndex ? "in index ✓" : "not in index ✗"}
      </text>
    </box>
    {entry.description === undefined ? null : (
      <text fg={C.textDim}>{clip(entry.description, DESC_MAX)}</text>
    )}
    <text> </text>
    <Field
      label="Size"
      value={`${fmtBytes(entry.size)} · ${entry.lines} lines`}
    />
    <Field label="Modified" value={fmtStarted(entry.mtime)} />
    <Field label="Frontmatter" value={entry.frontmatter.shape} />
    <Field label="File" value={clip(entry.fileName, DETAIL_W - 20)} />
    {entry.links.length > 0 ? (
      <box style={{ flexDirection: "column" }}>
        <text fg={C.textDim}>{`Links (${entry.links.length})`}</text>
        {entry.links.slice(0, LIST_CAP).map((link) => (
          <text
            fg={C.info}
            key={`${link.line}:${link.rawTarget}`}
          >{`  ↗ ${clip(link.rawTarget, DETAIL_W - 8)}`}</text>
        ))}
        {entry.links.length > LIST_CAP ? (
          <text
            fg={C.textFaint}
          >{`  +${entry.links.length - LIST_CAP} more`}</text>
        ) : null}
      </box>
    ) : null}
    <text fg={C.textDim}>Body</text>
    <scrollbox focused={bodyFocused} style={{ flexGrow: 1, minHeight: 0 }}>
      <text fg={C.text}>{clip(entry.body, BODY_MAX)}</text>
    </scrollbox>
  </Panel>
);

/** The windowed list of header + entry rows for the left pane. */
const MemoryList = ({
  rows,
  win,
  index,
}: {
  readonly rows: readonly Row[];
  readonly win: { readonly start: number; readonly end: number };
  readonly index: number;
}) => (
  <box style={{ flexDirection: "column" }}>
    {rows.slice(win.start, win.end).map((row, i) => {
      const abs = win.start + i;
      return row.kind === "header" ? (
        <HeaderRow
          key={row.key}
          matched={row.matched}
          selected={abs === index}
          vault={row.vault}
        />
      ) : (
        <EntryRow entry={row.entry} key={row.key} selected={abs === index} />
      );
    })}
  </box>
);

/** Right pane: the selected entry's detail, or the selected vault's summary. */
const RightPane = ({
  selected,
  bodyFocused,
}: {
  readonly selected: Row | undefined;
  readonly bodyFocused: boolean;
}) => {
  if (selected?.kind === "entry") {
    return <EntryDetail bodyFocused={bodyFocused} entry={selected.entry} />;
  }
  if (selected?.kind === "header") {
    return <VaultSummary vault={selected.vault} />;
  }
  return null;
};

/** The Memory screen. */
export const MemoryScreen = () => {
  const watch = useWatch();
  const vaultsQ = useQuery((c) => c.memory.allVaults(), [watch.memory]);
  const capsQ = useQuery((c) => c.capabilities.list(), []);
  const { setTyping } = useTyping();
  const { height } = useTerminalDimensions();

  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<TypeFilter>("all");
  const [searching, setSearching] = useState(false);
  const [bodyFocused, setBodyFocused] = useState(false);

  const all = vaultsQ.data;
  const rows = buildRows(all?.vaults ?? [], query.toLowerCase(), typeFilter);
  const listActive = !(searching || bodyFocused);
  const [index, setIndex] = useListSelection(rows.length, listActive);
  const selected = rows[index];

  const crud = (capsQ.data ?? []).find((c) => c.id === MEMORY_CRUD);
  const canWrite =
    crud?.perAgent[WRITE_AGENT as keyof typeof crud.perAgent]?.level ===
    "supported";

  const closeSearch = () => {
    setSearching(false);
    setTyping(false);
  };

  // `Tab` only engages body focus for an entry row — a vault header shows a
  // summary with no focusable scrollbox, so navigation stays with the list.
  useMemoryKeys({
    searching,
    selectedIsEntry: selected?.kind === "entry",
    onOpenSearch: () => {
      setSearching(true);
      setTyping(true);
      setBodyFocused(false);
    },
    onCloseSearch: closeSearch,
    onCycleType: () => {
      setTypeFilter(nextTypeFilter(typeFilter));
      setIndex(0);
    },
    onToggleBody: () => setBodyFocused((b) => !b),
  });

  const visible = Math.max(MIN_ROWS, height - CHROME_ROWS);
  const win = windowSlice(index, rows.length, visible);
  const noProjects = all !== undefined && all.projects.length === 0;
  const blocked = !(vaultsQ.loading || vaultsQ.error || noProjects);

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 0 }}>
      <box style={{ flexDirection: "row", gap: 1, height: 1 }}>
        <text fg={C.accent}>/</text>
        {searching ? (
          <input
            focused
            onInput={setQuery}
            onSubmit={closeSearch}
            placeholder="filter slug · name · description · body"
            style={{ flexGrow: 1 }}
          />
        ) : (
          <text fg={query === "" ? C.textFaint : C.text}>
            {query === "" ? "search (press /)" : query}
          </text>
        )}
        <text fg={C.textDim}>{`type:${typeFilter}`}</text>
      </box>

      <box style={{ flexDirection: "row", flexGrow: 1, minHeight: 0, gap: 1 }}>
        <Panel
          flexGrow={1}
          focused={!bodyFocused}
          title={`Memory (${rows.length})`}
        >
          {vaultsQ.loading && all === undefined ? <Loading /> : null}
          {vaultsQ.error ? <ErrorLine error={vaultsQ.error} /> : null}
          {noProjects ? <Empty label="No memories found." /> : null}
          {blocked && rows.length === 0 ? (
            <Empty label="No memories match the current filters." />
          ) : null}
          {rows.length > 0 ? (
            <MemoryList index={index} rows={rows} win={win} />
          ) : null}
        </Panel>
        <RightPane bodyFocused={bodyFocused} selected={selected} />
      </box>

      <text fg={C.textFaint}>
        {`editing available in the web app (peektrace serve) · memory.crud: ${canWrite ? "supported" : "read-only"}`}
      </text>
      <KeyHints
        hints={[
          ["↑↓/jk", "move"],
          ["/", "search"],
          ["t", "type"],
          ["tab", bodyFocused ? "list" : "body"],
          ["q", "quit"],
        ]}
      />
    </box>
  );
};
