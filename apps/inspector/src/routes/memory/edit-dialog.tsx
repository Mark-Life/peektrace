/** Edit-memory form (`memory.update`) with compare-and-swap conflict handling.
 *
 * Pre-fills frontmatter (name/description/type) + body and sends the loaded
 * `mtimeMs` as `expectedMtime`. If the file changed under us the server returns
 * a typed `FileChangedError`; instead of clobbering, we surface a reload-or-
 * overwrite choice: "Reload" refreshes from disk (discarding edits), "Overwrite"
 * re-sends without the CAS guard.
 */
import { useAtomSet } from "@effect-atom/atom-react";
import type { MemoryEntry } from "@workspace/core/services/memory/types";
import { MEMORY_TYPES } from "@workspace/core/services/memory/types";
import { Button } from "@workspace/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@workspace/ui/components/dialog";
import { Input } from "@workspace/ui/components/input";
import { Label } from "@workspace/ui/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { Textarea } from "@workspace/ui/components/textarea";
import { Exit } from "effect";
import { AlertTriangleIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { updateMemoryAtom } from "../../lib/memory-atoms";
import { wireErrorMessage, wireErrorOf } from "../../lib/wire-error";

/** Edit dialog for a single memory entry. */
export const EditDialog = ({
  project,
  entry,
  open,
  onOpenChange,
  onDone,
}: {
  readonly project: string;
  readonly entry: MemoryEntry;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly onDone: () => void;
}) => {
  const update = useAtomSet(updateMemoryAtom, { mode: "promiseExit" });
  const [description, setDescription] = useState(entry.description ?? "");
  const [type, setType] = useState(entry.type ?? "project");
  const [body, setBody] = useState(entry.body);
  const [error, setError] = useState<string | undefined>();
  const [conflict, setConflict] = useState(false);
  const [busy, setBusy] = useState(false);

  const send = async (guard: boolean) => {
    setBusy(true);
    setError(undefined);
    const exit = await update({
      payload: {
        project,
        name: entry.slug,
        frontmatter: { description, type },
        body,
        ...(guard ? { expectedMtime: entry.mtimeMs } : {}),
      },
    });
    setBusy(false);
    if (Exit.isSuccess(exit)) {
      toast.success(`Saved ${entry.slug}`);
      onDone();
      onOpenChange(false);
      return;
    }
    const err = wireErrorOf(exit);
    if (err?._tag === "FileChangedError") {
      setConflict(true);
      return;
    }
    setError(err ? wireErrorMessage(err) : "Save failed.");
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="sm:max-w-lg" data-testid="edit-dialog">
        <DialogHeader>
          <DialogTitle>Edit {entry.slug}</DialogTitle>
          <DialogDescription>
            Re-serializes frontmatter + body and keeps the index line in sync.
          </DialogDescription>
        </DialogHeader>
        {conflict ? (
          <div
            className="flex flex-col gap-3 rounded-md border border-amber-500/40 bg-amber-500/10 p-4"
            data-testid="cas-conflict"
          >
            <div className="flex items-center gap-2 font-medium text-amber-300">
              <AlertTriangleIcon className="size-4" />
              File changed on disk
            </div>
            <p className="text-muted-foreground text-sm">
              {entry.fileName} was modified after you opened it. Reload to pull
              the latest (your edits are discarded) or overwrite to force-save.
            </p>
            <div className="flex justify-end gap-2">
              <Button
                data-testid="cas-reload"
                onClick={() => {
                  onDone();
                  onOpenChange(false);
                }}
                variant="outline"
              >
                Reload from disk
              </Button>
              <Button
                data-testid="cas-overwrite"
                disabled={busy}
                onClick={() => send(false)}
                variant="destructive"
              >
                Overwrite
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="edit-desc">Description</Label>
              <Input
                data-testid="edit-description"
                id="edit-desc"
                onChange={(e) => setDescription(e.target.value)}
                value={description}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-type">Type</Label>
              <Select onValueChange={setType} value={type}>
                <SelectTrigger data-testid="edit-type" id="edit-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MEMORY_TYPES.map((t) => (
                    <SelectItem key={t} value={t}>
                      {t}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-body">Body</Label>
              <Textarea
                className="min-h-40 font-mono text-xs"
                data-testid="edit-body"
                id="edit-body"
                onChange={(e) => setBody(e.target.value)}
                value={body}
              />
            </div>
            {error ? (
              <p className="text-red-400 text-sm" data-testid="edit-error">
                {error}
              </p>
            ) : null}
          </div>
        )}
        {conflict ? null : (
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              Cancel
            </Button>
            <Button
              data-testid="edit-submit"
              disabled={busy}
              onClick={() => send(true)}
              type="button"
            >
              {busy ? "Saving…" : "Save changes"}
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
};
