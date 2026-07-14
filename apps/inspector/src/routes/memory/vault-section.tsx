/** One project's full memory surface, used as an accordion panel body.
 *
 * Composes the parity forensics — budget gauge, type donut, index↔files diff,
 * link graph — above the sortable browse table, plus the per-project Create
 * action and the edit/delete dialogs. All write paths are hidden unless
 * `canWrite` (the capability gate). Link clicks and graph-node clicks expand the
 * target row in place.
 */
import type {
  MemoryEntry,
  MemoryVault,
} from "@workspace/core/services/memory/types";
import { Button } from "@workspace/ui/components/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@workspace/ui/components/card";
import { BudgetGauge } from "@workspace/viz/components/budget-gauge";
import { LinkGraph } from "@workspace/viz/components/link-graph";
import { TypeDonut } from "@workspace/viz/components/type-donut";
import { PlusIcon } from "lucide-react";
import { type ReactNode, useState } from "react";
import { BrowseTable } from "./browse-table";
import { CreateDialog } from "./create-dialog";
import { DeleteDialog } from "./delete-dialog";
import { DiffPanel } from "./diff-panel";
import { EditDialog } from "./edit-dialog";

/** A small titled panel wrapper. */
const Panel = ({
  title,
  children,
}: {
  readonly title: string;
  readonly children: ReactNode;
}) => (
  <Card>
    <CardHeader className="pb-2">
      <CardTitle className="text-sm">{title}</CardTitle>
    </CardHeader>
    <CardContent>{children}</CardContent>
  </Card>
);

/** Render the full forensic + CRUD surface for one vault. */
export const VaultSection = ({
  vault,
  entries,
  canWrite,
  onRefresh,
}: {
  readonly vault: MemoryVault;
  readonly entries: readonly MemoryEntry[];
  readonly canWrite: boolean;
  readonly onRefresh: () => void;
}) => {
  const [highlight, setHighlight] = useState<string | undefined>();
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<MemoryEntry | undefined>();
  const [deleting, setDeleting] = useState<MemoryEntry | undefined>();

  return (
    <div className="flex flex-col gap-4" data-testid="vault-section">
      <div className="grid gap-4 lg:grid-cols-3">
        <Panel title="Index budget">
          <BudgetGauge budget={vault.budget} />
        </Panel>
        <Panel title="Types">
          <TypeDonut typeCounts={vault.typeCounts} />
        </Panel>
        <Panel title="Index ↔ files">
          <DiffPanel diff={vault.diff} />
        </Panel>
      </div>
      <div className="grid gap-4 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="font-medium text-sm">
              {entries.length} of {vault.entries.length} memories
            </h3>
            {canWrite ? (
              <Button
                data-testid="create-open"
                onClick={() => setCreating(true)}
                size="sm"
              >
                <PlusIcon className="size-4" /> New memory
              </Button>
            ) : null}
          </div>
          <BrowseTable
            actions={{
              canWrite,
              onEdit: setEditing,
              onDelete: setDeleting,
              onNavigate: setHighlight,
            }}
            entries={entries}
            highlightSlug={highlight}
            onExpandChange={setHighlight}
          />
        </div>
        <Panel title="Link graph">
          <LinkGraph graph={vault.graph} onSelect={setHighlight} />
        </Panel>
      </div>

      <CreateDialog
        onDone={onRefresh}
        onOpenChange={setCreating}
        open={creating}
        project={vault.slug}
      />
      {editing ? (
        <EditDialog
          entry={editing}
          onDone={onRefresh}
          onOpenChange={(o) => !o && setEditing(undefined)}
          open={Boolean(editing)}
          project={vault.slug}
        />
      ) : null}
      {deleting ? (
        <DeleteDialog
          entry={deleting}
          onDone={onRefresh}
          onOpenChange={(o) => !o && setDeleting(undefined)}
          open={Boolean(deleting)}
          project={vault.slug}
        />
      ) : null}
    </div>
  );
};
