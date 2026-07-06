/** Create-memory form (`memory.create`).
 *
 * Collects name (kebab) / description / type / body, writes via the create
 * mutation, then calls `onDone` so the explorer refreshes the vault — which
 * re-validates the budget gauge, diff and graph from disk. Typed failures
 * (e.g. duplicate name, validation) surface inline; success toasts + closes.
 */
import { useAtomSet } from "@effect-atom/atom-react";
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
import { useState } from "react";
import { toast } from "sonner";
import { createMemoryAtom } from "../../lib/memory-atoms";
import { wireErrorMessage, wireErrorOf } from "../../lib/wire-error";

/** Create dialog bound to one project slug. */
export const CreateDialog = ({
  project,
  open,
  onOpenChange,
  onDone,
}: {
  readonly project: string;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly onDone: () => void;
}) => {
  const create = useAtomSet(createMemoryAtom, { mode: "promiseExit" });
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [type, setType] = useState<string>("project");
  const [body, setBody] = useState("");
  const [error, setError] = useState<string | undefined>();
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(undefined);
    const exit = await create({
      payload: { project, name, description, type, body },
    });
    setBusy(false);
    if (Exit.isSuccess(exit)) {
      toast.success(`Created ${name}`);
      onDone();
      onOpenChange(false);
      setName("");
      setDescription("");
      setBody("");
      return;
    }
    const err = wireErrorOf(exit);
    setError(err ? wireErrorMessage(err) : "Create failed.");
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="sm:max-w-lg" data-testid="create-dialog">
        <DialogHeader>
          <DialogTitle>New memory</DialogTitle>
          <DialogDescription>
            Adds <code>{name || "<name>"}.md</code> and a MEMORY.md index line
            in <code>{project}</code>.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="mem-name">Name (kebab-case)</Label>
            <Input
              data-testid="create-name"
              id="mem-name"
              onChange={(e) => setName(e.target.value)}
              placeholder="api-conventions"
              value={name}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="mem-desc">Description</Label>
            <Input
              data-testid="create-description"
              id="mem-desc"
              onChange={(e) => setDescription(e.target.value)}
              placeholder="One-line hook for the index"
              value={description}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="mem-type">Type</Label>
            <Select onValueChange={setType} value={type}>
              <SelectTrigger data-testid="create-type" id="mem-type">
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
            <Label htmlFor="mem-body">Body (markdown)</Label>
            <Textarea
              className="min-h-32 font-mono text-xs"
              data-testid="create-body"
              id="mem-body"
              onChange={(e) => setBody(e.target.value)}
              placeholder="# Title&#10;&#10;Use [[other-memory]] to link."
              value={body}
            />
          </div>
          {error ? (
            <p className="text-red-400 text-sm" data-testid="create-error">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button
            onClick={() => onOpenChange(false)}
            type="button"
            variant="ghost"
          >
            Cancel
          </Button>
          <Button
            data-testid="create-submit"
            disabled={busy || name.length === 0}
            onClick={submit}
            type="button"
          >
            {busy ? "Creating…" : "Create memory"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
