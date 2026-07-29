/** Settings screen — types, pure transforms, and presentational rows.
 *
 * Split from `settings.tsx` so the screen file stays a thin controller: this
 * module holds the editable-row model, the seed/assemble transforms between it
 * and the wire `PeektraceSettings`, and the leaf views (field cell, per-agent
 * section, status line, conflict banner). No state or IO lives here.
 */
import { AGENT_IDS } from "@workspace/core/services/agent-id";
import type {
  PeektraceSettings,
  RootEntry,
} from "@workspace/core/services/settings";
import { Empty } from "../components";
import { AGENT_LABEL, C, clip } from "../theme";

/** An agent id in the settings keyset. */
export type AgentId = (typeof AGENT_IDS)[number];

/** One editable root row; `id` is a stable react/selection key. */
export interface EditRow {
  readonly id: string;
  readonly label: string;
  readonly path: string;
}

/** Which field of which row is currently in text-edit mode. */
export interface EditTarget {
  readonly agent: AgentId;
  readonly field: "label" | "path";
  readonly rowId: string;
}

/** A flat navigation entry: a root row, or an agent's "add root" affordance. */
export type NavEntry =
  | { readonly kind: "root"; readonly agent: AgentId; readonly rowId: string }
  | { readonly kind: "add"; readonly agent: AgentId };

/** Save/refresh status surfaced under the banner. */
export type Status =
  | { readonly kind: "idle" }
  | { readonly kind: "saving" }
  | { readonly kind: "saved" }
  | { readonly kind: "conflict" }
  | { readonly kind: "error"; readonly message: string };

const PATH_W = 38;
const LABEL_W = 16;
const MSG_MAX = 160;
const PATH_PLACEHOLDER = "~/work/.claude";
const LABEL_PLACEHOLDER = "label";

/** Seed editable rows from a loaded settings file (one bucket per agent). */
export const seedRows = (
  settings: PeektraceSettings,
  nextId: () => string
): Record<AgentId, EditRow[]> => {
  const out = {} as Record<AgentId, EditRow[]>;
  for (const agent of AGENT_IDS) {
    const list = settings.roots?.[agent] ?? [];
    out[agent] = list.map((r) => ({
      id: nextId(),
      path: r.path,
      label: r.label ?? "",
    }));
  }
  return out;
};

/** Trim, drop empty-path rows, omit empty labels, omit empty agents. */
export const assemble = (
  rows: Record<AgentId, EditRow[]>
): PeektraceSettings => {
  const roots: Partial<Record<AgentId, RootEntry[]>> = {};
  for (const agent of AGENT_IDS) {
    const entries = (rows[agent] ?? [])
      .map((r) => ({ path: r.path.trim(), label: r.label.trim() }))
      .filter((r) => r.path !== "")
      .map(
        (r): RootEntry =>
          r.label === "" ? { path: r.path } : { path: r.path, label: r.label }
      );
    if (entries.length > 0) {
      roots[agent] = entries;
    }
  }
  return Object.keys(roots).length > 0 ? { roots } : {};
};

/** The tagged-error discriminant of a rejected wire failure, if any. */
export const tagOf = (err: unknown): string | undefined =>
  typeof err === "object" && err !== null && "_tag" in err
    ? String((err as { _tag: unknown })._tag)
    : undefined;

/** A human message for a rejected call. */
export const msgOf = (err: unknown): string =>
  err instanceof Error ? err.message : String(err);

/** Build the flat, navigable entry list across every agent. */
export const buildEntries = (rows: Record<AgentId, EditRow[]>): NavEntry[] => {
  const entries: NavEntry[] = [];
  for (const agent of AGENT_IDS) {
    for (const row of rows[agent] ?? []) {
      entries.push({ kind: "root", agent, rowId: row.id });
    }
    entries.push({ kind: "add", agent });
  }
  return entries;
};

/** Foreground for a non-edited field value (faint when empty). */
const cellColor = (value: string, selected: boolean): string => {
  if (value === "") {
    return C.textFaint;
  }
  return selected ? C.text : C.textDim;
};

/** One field cell: a focused input while edited, else the value or placeholder. */
const FieldCell = ({
  width,
  value,
  placeholder,
  editing,
  draft,
  selected,
  onInput,
  onSubmit,
}: {
  readonly width: number;
  readonly value: string;
  readonly placeholder: string;
  readonly editing: boolean;
  readonly draft: string;
  readonly selected: boolean;
  readonly onInput: (v: string) => void;
  readonly onSubmit: () => void;
}) => {
  if (editing) {
    return (
      <box style={{ width }}>
        <input
          focused
          onInput={onInput}
          onSubmit={onSubmit}
          placeholder={placeholder}
          style={{ flexGrow: 1 }}
          value={draft}
        />
      </box>
    );
  }
  const shown = value === "" ? placeholder : clip(value, width - 1);
  return (
    <box style={{ width }}>
      <text fg={cellColor(value, selected)}>{shown}</text>
    </box>
  );
};

/** One agent's labelled block: its root rows plus an "add root" line. */
export const AgentSection = ({
  agent,
  rows,
  active,
  editing,
  draft,
  onInput,
  onSubmit,
}: {
  readonly agent: AgentId;
  readonly rows: readonly EditRow[];
  readonly active: NavEntry | undefined;
  readonly editing: EditTarget | null;
  readonly draft: string;
  readonly onInput: (v: string) => void;
  readonly onSubmit: () => void;
}) => {
  const addSelected = active?.kind === "add" && active.agent === agent;
  return (
    <box style={{ flexDirection: "column" }}>
      <text fg={C.accent}>{AGENT_LABEL[agent] ?? agent}</text>
      {rows.length === 0 ? <Empty label="  No extra roots." /> : null}
      {rows.map((row) => {
        const selected =
          active?.kind === "root" &&
          active.agent === agent &&
          active.rowId === row.id;
        return (
          <box key={row.id} style={{ flexDirection: "row", gap: 1 }}>
            <text fg={selected ? C.primary : C.textFaint}>
              {selected ? "›" : " "}
            </text>
            <FieldCell
              draft={draft}
              editing={editing?.rowId === row.id && editing.field === "path"}
              onInput={onInput}
              onSubmit={onSubmit}
              placeholder={PATH_PLACEHOLDER}
              selected={selected}
              value={row.path}
              width={PATH_W}
            />
            <FieldCell
              draft={draft}
              editing={editing?.rowId === row.id && editing.field === "label"}
              onInput={onInput}
              onSubmit={onSubmit}
              placeholder={LABEL_PLACEHOLDER}
              selected={selected}
              value={row.label}
              width={LABEL_W}
            />
            {selected && editing === null ? (
              <text fg={C.textFaint}>d:del</text>
            ) : null}
          </box>
        );
      })}
      <text fg={addSelected ? C.primary : C.textFaint}>
        {`${addSelected ? "› " : "  "}+ add root`}
      </text>
    </box>
  );
};

/** One-line save/refresh status under the banner. */
export const StatusLine = ({ status }: { readonly status: Status }) => {
  switch (status.kind) {
    case "saving":
      return <text fg={C.textDim}>Saving…</text>;
    case "saved":
      return (
        <text fg={C.good}>Saved — restart `peektrace serve` to apply.</text>
      );
    case "error":
      return <text fg={C.bad}>{`✗ ${clip(status.message, MSG_MAX)}`}</text>;
    default:
      return <text fg={C.textFaint}> </text>;
  }
};

/** Amber compare-and-swap conflict banner with reload / overwrite actions. */
export const ConflictBanner = () => (
  <box
    borderColor={C.warn}
    borderStyle="rounded"
    style={{ flexDirection: "column", padding: 0, paddingLeft: 1 }}
  >
    <text fg={C.warn}>settings.json changed on disk.</text>
    <box style={{ flexDirection: "row", gap: 2 }}>
      <text fg={C.accent}>r</text>
      <text fg={C.textFaint}>reload (discard edits)</text>
      <text fg={C.accent}>o</text>
      <text fg={C.textFaint}>overwrite</text>
    </box>
  </box>
);
