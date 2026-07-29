/** Settings section — edit `~/.peektrace/settings.json` (extra agent roots).
 *
 * Reads the file fresh via `settings.get`, renders a per-agent list of extra
 * config roots (path + optional label), and writes back with `settings.update`
 * (CAS on mtime). Because the running server resolves agent roots once at boot,
 * a banner makes clear that saved changes only affect the scanned session list
 * after `peektrace serve` restarts.
 */
import {
  useAtomRefresh,
  useAtomSet,
  useAtomValue,
} from "@effect-atom/atom-react";
import { AGENT_IDS, type AgentId } from "@workspace/core/services/agent-id";
import type { PeektraceSettings } from "@workspace/core/services/settings";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { Button } from "@workspace/ui/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@workspace/ui/components/card";
import { Input } from "@workspace/ui/components/input";
import { Label } from "@workspace/ui/components/label";
import { Exit } from "effect";
import {
  AlertTriangleIcon,
  InfoIcon,
  PlusIcon,
  SaveIcon,
  Trash2Icon,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { settingsAtom } from "../lib/atoms";
import { ResultView } from "../lib/result-view";
import { updateSettingsAtom } from "../lib/settings-atoms";
import { wireErrorMessage, wireErrorOf } from "../lib/wire-error";

/** Human label per agent id. */
const AGENT_LABEL: Record<AgentId, string> = {
  claude: "Claude",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

/** One editable extra-root row. */
interface Row {
  readonly label: string;
  readonly path: string;
}

type RowsByAgent = Record<AgentId, Row[]>;

/** Seed the editable per-agent rows from the loaded settings. */
const seedRows = (settings: PeektraceSettings): RowsByAgent =>
  Object.fromEntries(
    AGENT_IDS.map((agent) => [
      agent,
      (settings.roots?.[agent] ?? []).map((entry) => ({
        path: entry.path,
        label: entry.label ?? "",
      })),
    ])
  ) as RowsByAgent;

/** Collapse the editable rows back into a `PeektraceSettings` (drop empties). */
const assemble = (rows: RowsByAgent): PeektraceSettings => {
  const roots: Partial<Record<AgentId, { path: string; label?: string }[]>> =
    {};
  for (const agent of AGENT_IDS) {
    const cleaned = rows[agent]
      .map((r) => {
        const path = r.path.trim();
        const label = r.label.trim();
        return label ? { path, label } : { path };
      })
      .filter((r) => r.path !== "");
    if (cleaned.length > 0) {
      roots[agent] = cleaned;
    }
  }
  return Object.keys(roots).length > 0 ? { roots } : {};
};

/** Per-agent card: a list of extra roots with add/remove + path/label inputs. */
const AgentRoots = ({
  agent,
  rows,
  onChange,
}: {
  readonly agent: AgentId;
  readonly rows: Row[];
  readonly onChange: (rows: Row[]) => void;
}) => {
  const setRow = (index: number, patch: Partial<Row>) =>
    onChange(rows.map((r, i) => (i === index ? { ...r, ...patch } : r)));
  const removeRow = (index: number) =>
    onChange(rows.filter((_, i) => i !== index));
  const addRow = () => onChange([...rows, { path: "", label: "" }]);

  return (
    <Card data-testid={`settings-agent-${agent}`}>
      <CardHeader>
        <CardTitle>{AGENT_LABEL[agent]}</CardTitle>
        <CardDescription>
          Extra config dirs to scan alongside the default. The transcript root
          is derived from each path per agent layout.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {rows.length === 0 ? (
          <p className="text-muted-foreground text-sm">No extra roots.</p>
        ) : (
          rows.map((row, index) => (
            <div
              className="flex items-end gap-2"
              // biome-ignore lint/suspicious/noArrayIndexKey: rows have no stable id; index is the edit position.
              key={index}
            >
              <div className="grid flex-1 gap-1.5">
                <Label htmlFor={`${agent}-path-${index}`}>Path</Label>
                <Input
                  data-testid={`settings-path-${agent}-${index}`}
                  id={`${agent}-path-${index}`}
                  onChange={(e) => setRow(index, { path: e.target.value })}
                  placeholder="~/work/.claude"
                  value={row.path}
                />
              </div>
              <div className="grid w-40 gap-1.5">
                <Label htmlFor={`${agent}-label-${index}`}>Label</Label>
                <Input
                  data-testid={`settings-label-${agent}-${index}`}
                  id={`${agent}-label-${index}`}
                  onChange={(e) => setRow(index, { label: e.target.value })}
                  placeholder="work"
                  value={row.label}
                />
              </div>
              <Button
                aria-label="Remove root"
                data-testid={`settings-remove-${agent}-${index}`}
                onClick={() => removeRow(index)}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Trash2Icon />
              </Button>
            </div>
          ))
        )}
        <div>
          <Button
            data-testid={`settings-add-${agent}`}
            onClick={addRow}
            size="sm"
            type="button"
            variant="outline"
          >
            <PlusIcon />
            Add root
          </Button>
        </div>
      </CardContent>
    </Card>
  );
};

/** The editor form, seeded from one loaded snapshot (re-keyed on refresh). */
const SettingsEditor = ({
  settings,
  mtimeMs,
  path,
}: {
  readonly settings: PeektraceSettings;
  readonly mtimeMs: number;
  readonly path: string;
}) => {
  const save = useAtomSet(updateSettingsAtom, { mode: "promiseExit" });
  const refresh = useAtomRefresh(settingsAtom);
  const [rows, setRows] = useState<RowsByAgent>(() => seedRows(settings));
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);

  // `guard` on → CAS on the loaded mtime; off → force-overwrite a conflict.
  const send = async (guard: boolean) => {
    setBusy(true);
    const exit = await save({
      payload: {
        settings: assemble(rows),
        ...(guard ? { expectedMtime: mtimeMs } : {}),
      },
    });
    setBusy(false);
    if (Exit.isSuccess(exit)) {
      setConflict(false);
      toast.success("Settings saved", {
        description: "Restart `peektrace serve` to apply to the session list.",
      });
      refresh();
      return;
    }
    const err = wireErrorOf(exit);
    if (err?._tag === "FileChangedError") {
      // Keep the edits mounted so the user can reload or overwrite — never
      // silently discard what they typed.
      setConflict(true);
      return;
    }
    toast.error(err ? wireErrorMessage(err) : "Save failed.");
  };

  return (
    <div className="flex flex-col gap-4" data-testid="settings-editor">
      <Alert>
        <InfoIcon />
        <AlertTitle>Changes apply after a restart</AlertTitle>
        <AlertDescription>
          Saved to <code>{path}</code>. The running server scans the roots it
          read at startup, so restart <code>peektrace serve</code> for new roots
          to appear in the session list.
        </AlertDescription>
      </Alert>

      {conflict ? (
        <div
          className="flex flex-col gap-3 rounded-md border border-amber-500/40 bg-amber-500/10 p-4"
          data-testid="settings-cas-conflict"
        >
          <div className="flex items-center gap-2 font-medium text-amber-300">
            <AlertTriangleIcon className="size-4" />
            settings.json changed on disk
          </div>
          <p className="text-muted-foreground text-sm">
            The file was modified after you loaded it. Reload to pull the latest
            (your edits are discarded) or overwrite to force-save what you have.
          </p>
          <div className="flex justify-end gap-2">
            <Button
              data-testid="settings-cas-reload"
              onClick={() => refresh()}
              type="button"
              variant="outline"
            >
              Reload from disk
            </Button>
            <Button
              data-testid="settings-cas-overwrite"
              disabled={busy}
              onClick={() => send(false)}
              type="button"
              variant="destructive"
            >
              Overwrite
            </Button>
          </div>
        </div>
      ) : null}

      {AGENT_IDS.map((agent) => (
        <AgentRoots
          agent={agent}
          key={agent}
          onChange={(next) => setRows((prev) => ({ ...prev, [agent]: next }))}
          rows={rows[agent]}
        />
      ))}

      <div className="flex justify-end">
        <Button
          data-testid="settings-save"
          disabled={busy}
          onClick={() => send(true)}
          type="button"
        >
          <SaveIcon />
          {busy ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
};

/** Settings section route. */
export const SettingsRoute = () => {
  const result = useAtomValue(settingsAtom);
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-4">
      <div>
        <h1 className="font-semibold text-lg">Settings</h1>
        <p className="text-muted-foreground text-sm">
          Point peektrace at extra agent config dirs — e.g. a separate work
          account — to scan them in parallel.
        </p>
      </div>
      <ResultView result={result}>
        {(value) => (
          <SettingsEditor
            key={value.mtimeMs}
            mtimeMs={value.mtimeMs}
            path={value.path}
            settings={value.settings}
          />
        )}
      </ResultView>
    </div>
  );
};
