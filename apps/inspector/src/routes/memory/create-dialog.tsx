/** Create-memory form (`memory.create`).
 *
 * Collects name (kebab) / description / type / body, writes via the create
 * mutation, then calls `onDone` with the project slug so the explorer refreshes
 * the vault — which re-validates the budget gauge, diff and graph from disk.
 * The target is either one fixed project or picked in the form. Typed failures
 * (e.g. duplicate name, validation) surface inline; success toasts + closes.
 */
import { useAtomSet } from "@effect-atom/atom-react";
import {
  MEMORY_TYPES,
  type ProjectLabel,
} from "@workspace/core/services/memory/types";
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
import { useRef, useState } from "react";
import { toast } from "sonner";
import { createMemoryAtom } from "../../lib/memory-atoms";
import { wireErrorMessage, wireErrorOf } from "../../lib/wire-error";
import type { ProjectOption } from "./project-options";
import { ProjectPicker } from "./project-picker";

/** Where the new memory goes: one fixed project, or chosen in the form. */
type CreateTarget =
  | { readonly kind: "fixed"; readonly slug: string }
  | { readonly kind: "pick"; readonly labels: readonly ProjectLabel[] };

/** Create dialog for a fixed project or with a project field. */
export const CreateDialog = ({
  target,
  open,
  onOpenChange,
  onDone,
}: {
  readonly target: CreateTarget;
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly onDone: (project: string) => void;
}) => {
  const create = useAtomSet(createMemoryAtom, { mode: "promiseExit" });
  const [picked, setPicked] = useState<ProjectOption | undefined>();
  const project = target.kind === "fixed" ? target.slug : picked?.slug;
  const projectLabel = target.kind === "fixed" ? target.slug : picked?.label;
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [type, setType] = useState<string>("project");
  const [body, setBody] = useState("");
  const [error, setError] = useState<string | undefined>();
  const [busy, setBusy] = useState(false);
  // In pick mode the caller moves focus to the new memory after a create.
  const created = useRef(false);

  const close = (next: boolean) => {
    if (!next) {
      setPicked(undefined);
      setError(undefined);
    }
    onOpenChange(next);
  };

  const submit = async () => {
    if (project === undefined) {
      return;
    }
    setBusy(true);
    setError(undefined);
    const exit = await create({
      payload: { project, name, description, type, body },
    });
    setBusy(false);
    if (Exit.isSuccess(exit)) {
      toast.success(`Created ${name}`);
      created.current = target.kind === "pick";
      close(false);
      onDone(project);
      setName("");
      setDescription("");
      setBody("");
      return;
    }
    const err = wireErrorOf(exit);
    setError(err ? wireErrorMessage(err) : "Create failed.");
  };

  return (
    <Dialog onOpenChange={close} open={open}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="create-dialog"
        onCloseAutoFocus={(e) => {
          if (created.current) {
            e.preventDefault();
            created.current = false;
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>New memory</DialogTitle>
          <DialogDescription>
            Adds <code>{name || "<name>"}.md</code> and a MEMORY.md index line
            in <code>{projectLabel ?? "<project>"}</code>.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          {target.kind === "pick" ? (
            <ProjectPicker
              labels={target.labels}
              onChange={setPicked}
              value={picked}
            />
          ) : null}
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
          <Button onClick={() => close(false)} type="button" variant="ghost">
            Cancel
          </Button>
          <Button
            data-testid="create-submit"
            disabled={busy || name.length === 0 || project === undefined}
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
