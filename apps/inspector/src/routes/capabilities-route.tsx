/** Capabilities matrix (Phase 6) + cell detail drawer (Phase 9.1).
 *
 * Rows = feature capabilities (grouped); columns = the four tracked agents.
 * Each cell shows the per-agent `SupportLevel`, colored. Clicking a cell opens a
 * detail sheet with the capability title/description + that agent's `perAgent`
 * note (e.g. "memory edit: Claude only; Codex sessions: planned") and the
 * SupportLevel legend.
 */
import { useAtomValue } from "@effect-atom/atom-react";
import type { Capability } from "@workspace/rpc/contract";
import { Badge } from "@workspace/ui/components/badge";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@workspace/ui/components/sheet";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { cn } from "@workspace/ui/lib/utils";
import { Fragment, useState } from "react";
import { capabilitiesAtom } from "../lib/atoms";
import { ResultView } from "../lib/result-view";

/** A per-agent support cell, narrowed from the contract `Capability`. */
type Support = Capability["perAgent"][keyof Capability["perAgent"]];

/** Column order for the matrix (also the agent-id set). */
const AGENT_COLUMNS = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "pi", label: "Pi" },
  { id: "opencode", label: "OpenCode" },
] as const;

/** Tailwind classes per support level. */
const LEVEL_STYLES: Record<Support["level"], string> = {
  supported: "bg-emerald-500/15 text-emerald-400 border-emerald-500/30",
  partial: "bg-amber-500/15 text-amber-400 border-amber-500/30",
  planned: "bg-sky-500/15 text-sky-400 border-sky-500/30",
  unsupported: "bg-muted text-muted-foreground border-border",
};

/** Plain-language gloss per support level (shown in the legend + drawer). */
const LEVEL_GLOSS: Record<Support["level"], string> = {
  supported: "Built and working today.",
  partial: "Partially built — some paths work.",
  planned: "On the roadmap, not yet built.",
  unsupported: "Not applicable / no plan.",
};

/** The selected cell carried into the detail sheet. */
interface Selection {
  readonly agentId: (typeof AGENT_COLUMNS)[number]["id"];
  readonly agentLabel: string;
  readonly cap: Capability;
  readonly support: Support;
}

/** A support pill, with the level class applied. */
const Pill = ({ level }: { readonly level: Support["level"] }) => (
  <span
    className={cn(
      "inline-flex items-center rounded-full border px-2 py-0.5 font-medium text-xs capitalize",
      LEVEL_STYLES[level]
    )}
  >
    {level}
  </span>
);

/** Group capabilities by their `group` field, preserving first-seen order. */
const groupBy = (caps: readonly Capability[]) => {
  const groups = new Map<string, Capability[]>();
  for (const cap of caps) {
    const bucket = groups.get(cap.group) ?? [];
    bucket.push(cap);
    groups.set(cap.group, bucket);
  }
  return [...groups.entries()];
};

/** The matrix table; clicking a cell calls `onSelect`. */
const Matrix = ({
  caps,
  onSelect,
}: {
  readonly caps: readonly Capability[];
  readonly onSelect: (s: Selection) => void;
}) => (
  <div
    className="rounded-lg border border-border"
    data-testid="capability-matrix"
  >
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-[40%]">Capability</TableHead>
          {AGENT_COLUMNS.map((agent) => (
            <TableHead key={agent.id}>{agent.label}</TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {groupBy(caps).map(([group, rows]) => (
          <Fragment key={group}>
            <TableRow className="bg-muted/40 hover:bg-muted/40">
              <TableCell
                className="py-1.5 font-medium text-muted-foreground text-xs uppercase tracking-wide"
                colSpan={1 + AGENT_COLUMNS.length}
              >
                {group}
              </TableCell>
            </TableRow>
            {rows.map((cap) => (
              <TableRow key={cap.id}>
                <TableCell className="align-top">
                  <div className="font-medium text-sm">{cap.title}</div>
                  <div className="text-muted-foreground text-xs">
                    {cap.description}
                  </div>
                </TableCell>
                {AGENT_COLUMNS.map((agent) => (
                  <TableCell className="align-top" key={agent.id}>
                    <button
                      className="cursor-pointer rounded-sm focus:outline-none focus:ring-2 focus:ring-ring"
                      data-testid={`cap-cell-${cap.id}-${agent.id}`}
                      onClick={() =>
                        onSelect({
                          cap,
                          agentId: agent.id,
                          agentLabel: agent.label,
                          support: cap.perAgent[agent.id],
                        })
                      }
                      type="button"
                    >
                      <Pill level={cap.perAgent[agent.id].level} />
                    </button>
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </Fragment>
        ))}
      </TableBody>
    </Table>
  </div>
);

/** Legend mapping each level to its swatch + gloss. */
const Legend = () => (
  <div className="mt-4 flex flex-wrap gap-4 text-xs" data-testid="cap-legend">
    {(Object.keys(LEVEL_STYLES) as Support["level"][]).map((level) => (
      <span className="inline-flex items-center gap-1.5" key={level}>
        <Pill level={level} />
        <span className="text-muted-foreground">{LEVEL_GLOSS[level]}</span>
      </span>
    ))}
  </div>
);

/** The detail sheet for a selected capability × agent cell. */
const CellSheet = ({
  selection,
  onClose,
}: {
  readonly selection: Selection | null;
  readonly onClose: () => void;
}) => (
  <Sheet
    onOpenChange={(open) => (open ? null : onClose())}
    open={Boolean(selection)}
  >
    <SheetContent data-testid="cap-cell-sheet">
      {selection ? (
        <>
          <SheetHeader>
            <SheetTitle>{selection.cap.title}</SheetTitle>
            <SheetDescription>{selection.cap.description}</SheetDescription>
          </SheetHeader>
          <div className="flex flex-col gap-4 px-4">
            <div className="flex items-center gap-2">
              <Badge variant="secondary">{selection.agentLabel}</Badge>
              <Pill level={selection.support.level} />
            </div>
            <div>
              <div className="font-medium text-sm">Note</div>
              <p
                className="mt-1 text-muted-foreground text-sm"
                data-testid="cap-cell-note"
              >
                {selection.support.note ?? LEVEL_GLOSS[selection.support.level]}
              </p>
            </div>
            <div>
              <div className="font-medium text-sm">Legend</div>
              <Legend />
            </div>
          </div>
        </>
      ) : null}
    </SheetContent>
  </Sheet>
);

/** Capabilities section: live feature × agent support matrix + cell drawer. */
export const CapabilitiesRoute = () => {
  const result = useAtomValue(capabilitiesAtom);
  const [selection, setSelection] = useState<Selection | null>(null);
  return (
    <div>
      <ResultView result={result}>
        {(caps) => (
          <>
            <Matrix caps={caps} onSelect={setSelection} />
            <Legend />
            <CellSheet
              onClose={() => setSelection(null)}
              selection={selection}
            />
          </>
        )}
      </ResultView>
    </div>
  );
};
