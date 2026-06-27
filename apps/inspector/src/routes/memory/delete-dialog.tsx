/** Delete-memory confirm (`memory.delete`) with dangling-link warnings.
 *
 * Deletes the file + its MEMORY.md pointer line. The server returns the body
 * references that are now broken; we toast them so the user knows what to fix.
 */
import { useAtomSet } from "@effect-atom/atom-react";
import type { MemoryEntry } from "@workspace/core/services/memory/types";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@workspace/ui/components/alert-dialog";
import { Button } from "@workspace/ui/components/button";
import { Exit } from "effect";
import { useState } from "react";
import { toast } from "sonner";
import { deleteMemoryAtom } from "../../lib/memory-atoms";
import { wireErrorMessage, wireErrorOf } from "../../lib/wire-error";

/** Confirm dialog for deleting a single memory entry. */
export const DeleteDialog = ({
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
  const del = useAtomSet(deleteMemoryAtom, { mode: "promiseExit" });
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    setBusy(true);
    const exit = await del({ payload: { project, name: entry.slug } });
    setBusy(false);
    if (Exit.isSuccess(exit)) {
      const dangling = exit.value.dangling;
      if (dangling.length > 0) {
        toast.warning(
          `Deleted ${entry.slug} — ${dangling.length} reference${
            dangling.length === 1 ? "" : "s"
          } now dangling`,
          {
            description: dangling
              .map((d) => `${d.from} → ${d.target}`)
              .join(", "),
          }
        );
      } else {
        toast.success(`Deleted ${entry.slug}`);
      }
      onDone();
      onOpenChange(false);
      return;
    }
    const err = wireErrorOf(exit);
    toast.error(err ? wireErrorMessage(err) : "Delete failed.");
  };

  return (
    <AlertDialog onOpenChange={onOpenChange} open={open}>
      <AlertDialogContent data-testid="delete-dialog">
        <AlertDialogHeader>
          <AlertDialogTitle>Delete {entry.slug}?</AlertDialogTitle>
          <AlertDialogDescription>
            Removes <code>{entry.fileName}</code> and its MEMORY.md index line.
            Any <code>[[{entry.slug}]]</code> links elsewhere will be reported
            as dangling.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <Button
            data-testid="delete-confirm"
            disabled={busy}
            onClick={confirm}
            variant="destructive"
          >
            {busy ? "Deleting…" : "Delete"}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
};
