/** Settings screen — edit `~/.peektrace/settings.json` extra agent roots.
 *
 * The server resolves config roots once at boot, so this screen is a plain
 * editor over the settings file: per agent, a list of `{path, label}` rows a
 * user adds to point peektrace at additional config dirs (e.g. a work Claude
 * account beside the personal one). Local edits are seeded from the loaded file
 * and re-seeded whenever its mtime changes; Save is a compare-and-swap on that
 * mtime, with an amber conflict banner when the file moved under us. Mirrors the
 * web inspector's settings route, retuned for a narrow terminal. Row model and
 * leaf views live in `settings-rows`.
 */
import { useKeyboard } from "@opentui/react";
import { AGENT_IDS } from "@workspace/core/services/agent-id";
import { useEffect, useRef, useState } from "react";
import { KeyHints, Loading, Panel } from "../components";
import { useBridge, useQuery } from "../runtime";
import { C } from "../theme";
import { useTyping } from "../typing";
import { useListSelection } from "../use-list";
import {
  type AgentId,
  AgentSection,
  assemble,
  buildEntries,
  ConflictBanner,
  type EditRow,
  type EditTarget,
  msgOf,
  type NavEntry,
  type Status,
  StatusLine,
  seedRows,
  tagOf,
} from "./settings-rows";

/** The Settings screen. */
export const SettingsScreen = () => {
  const bridge = useBridge();
  const q = useQuery((c) => c.settings.get(), []);
  const { setTyping } = useTyping();

  const idRef = useRef(0);
  const nextId = () => `r${idRef.current++}`;
  const seededRef = useRef<number | null>(null);

  const [rows, setRows] = useState<Record<AgentId, EditRow[]>>(
    {} as Record<AgentId, EditRow[]>
  );
  const [mtime, setMtime] = useState<number | null>(null);
  const [editing, setEditing] = useState<EditTarget | null>(null);
  const [draft, setDraft] = useState("");
  const [status, setStatus] = useState<Status>({ kind: "idle" });

  // Re-seed local edit state whenever the loaded file's mtime changes; a Save
  // updates `seededRef` in place so its own returned result never re-seeds.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reseed keyed on data mtime only
  useEffect(() => {
    if (q.data === undefined || q.data.mtimeMs === seededRef.current) {
      return;
    }
    seededRef.current = q.data.mtimeMs;
    setMtime(q.data.mtimeMs);
    setRows(seedRows(q.data.settings, nextId));
    setEditing(null);
    setStatus({ kind: "idle" });
  }, [q.data]);

  useEffect(() => {
    setTyping(editing !== null);
  }, [editing, setTyping]);

  const entries = buildEntries(rows);
  const [index] = useListSelection(entries.length, editing === null);
  const active = entries[index];

  const setField = (
    agent: AgentId,
    rowId: string,
    field: "label" | "path",
    value: string
  ) => {
    setRows((prev) => ({
      ...prev,
      [agent]: (prev[agent] ?? []).map((r) =>
        r.id === rowId ? { ...r, [field]: value } : r
      ),
    }));
  };

  const addRow = (agent: AgentId) => {
    const id = nextId();
    setRows((prev) => ({
      ...prev,
      [agent]: [...(prev[agent] ?? []), { id, path: "", label: "" }],
    }));
    setDraft("");
    setEditing({ agent, rowId: id, field: "path" });
  };

  const removeRow = (agent: AgentId, rowId: string) => {
    setRows((prev) => ({
      ...prev,
      [agent]: (prev[agent] ?? []).filter((r) => r.id !== rowId),
    }));
  };

  const startEdit = (
    agent: AgentId,
    rowId: string,
    field: "label" | "path"
  ) => {
    const row = (rows[agent] ?? []).find((r) => r.id === rowId);
    setDraft(field === "path" ? (row?.path ?? "") : (row?.label ?? ""));
    setEditing({ agent, rowId, field });
  };

  const commitDraft = () => {
    if (editing !== null) {
      setField(editing.agent, editing.rowId, editing.field, draft);
    }
  };

  const submitField = () => {
    if (editing === null) {
      return;
    }
    commitDraft();
    if (editing.field === "path") {
      const row = (rows[editing.agent] ?? []).find(
        (r) => r.id === editing.rowId
      );
      setDraft(row?.label ?? "");
      setEditing({ ...editing, field: "label" });
    } else {
      setEditing(null);
    }
  };

  const toggleField = () => {
    if (editing === null) {
      return;
    }
    commitDraft();
    const next = editing.field === "path" ? "label" : "path";
    const row = (rows[editing.agent] ?? []).find((r) => r.id === editing.rowId);
    setDraft(next === "path" ? (row?.path ?? "") : (row?.label ?? ""));
    setEditing({ ...editing, field: next });
  };

  const send = async (guard: boolean) => {
    setStatus({ kind: "saving" });
    try {
      const res = await bridge.run((c) =>
        c.settings.update({
          settings: assemble(rows),
          ...(guard && mtime !== null ? { expectedMtime: mtime } : {}),
        })
      );
      seededRef.current = res.mtimeMs;
      setMtime(res.mtimeMs);
      setRows(seedRows(res.settings, nextId));
      setEditing(null);
      setStatus({ kind: "saved" });
    } catch (err) {
      setStatus(
        tagOf(err) === "FileChangedError"
          ? { kind: "conflict" }
          : { kind: "error", message: msgOf(err) }
      );
    }
  };

  const save = (guard: boolean) => {
    send(guard).catch(() => undefined);
  };

  const handleConflictKey = (name: string): boolean => {
    if (name === "r") {
      setStatus({ kind: "idle" });
      q.refetch();
      return true;
    }
    if (name === "o") {
      save(false);
      return true;
    }
    return false;
  };

  const handleEntryKey = (name: string, entry: NavEntry) => {
    switch (name) {
      case "return":
      case "e":
        if (entry.kind === "add") {
          addRow(entry.agent);
        } else {
          startEdit(entry.agent, entry.rowId, "path");
        }
        break;
      case "a":
        addRow(entry.agent);
        break;
      case "d":
      case "delete":
      case "backspace":
        if (entry.kind === "root") {
          removeRow(entry.agent, entry.rowId);
        }
        break;
      case "s":
        save(true);
        break;
      default:
        break;
    }
  };

  useKeyboard((key) => {
    if (editing !== null) {
      if (key.name === "escape") {
        setEditing(null);
      } else if (key.name === "tab") {
        toggleField();
      }
      return;
    }
    if (status.kind === "conflict" && handleConflictKey(key.name)) {
      return;
    }
    if (key.ctrl && key.name === "s") {
      save(true);
      return;
    }
    if (active !== undefined) {
      handleEntryKey(key.name, active);
    }
  });

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0, gap: 0 }}>
      <box
        borderColor={C.warn}
        borderStyle="rounded"
        style={{ flexDirection: "column", padding: 0, paddingLeft: 1 }}
        title="settings.json"
        titleColor={C.warn}
      >
        <text fg={C.warn}>
          Changes apply after a restart of `peektrace serve`
        </text>
        <text fg={C.textFaint}>{q.data?.path ?? "…"}</text>
      </box>
      <StatusLine status={status} />
      <Panel flexGrow={1} focused title="Agent roots">
        {q.loading && q.data === undefined ? <Loading /> : null}
        {AGENT_IDS.map((agent) => (
          <AgentSection
            active={active}
            agent={agent}
            draft={draft}
            editing={editing}
            key={agent}
            onInput={setDraft}
            onSubmit={submitField}
            rows={rows[agent] ?? []}
          />
        ))}
      </Panel>
      {status.kind === "conflict" ? <ConflictBanner /> : null}
      <KeyHints
        hints={[
          ["↑↓/jk", "move"],
          ["e/↵", "edit"],
          ["a", "add"],
          ["d", "remove"],
          ["^s", "save"],
          ["1-5", "section"],
          ["q", "quit"],
        ]}
      />
    </box>
  );
};
