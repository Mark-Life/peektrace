/** Capabilities matrix (Phase 6) + cell detail drawer (Phase 9.1).
 *
 * Rows = feature capabilities (grouped); columns = the four tracked agents.
 * Each cell shows the per-agent `SupportLevel`, colored. Clicking a cell opens a
 * detail sheet with the capability title/description + that agent's `perAgent`
 * note (e.g. "memory edit: Claude only; Codex sessions: planned") and the
 * SupportLevel legend.
 */
import { useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
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
import type { Capability } from "@workspace/view-core/api-types.gen";
import { Fragment, useState } from "react";

import { capabilitiesAtom } from "../lib/atoms";
import {
  AGENT_COLUMNS,
  cellNote,
  groupBy,
  LEVEL_GLOSS,
  LEVEL_STYLES,
  LEVELS,
} from "../lib/capability-matrix";
import type { Support } from "../lib/capability-matrix";
import { ResultView } from "../lib/result-view";

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
      "inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium capitalize",
      LEVEL_STYLES[level]
    )}
  >
    {level}
  </span>
);

/** The matrix table; clicking a cell calls `onSelect`. */
const Matrix = ({
  caps,
  onSelect,
}: {
  readonly caps: readonly Capability[];
  readonly onSelect: (s: Selection) => void;
}) => (
  <div
    className="border-border rounded-lg border"
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
                className="text-muted-foreground py-1.5 text-xs font-medium tracking-wide uppercase"
                colSpan={1 + AGENT_COLUMNS.length}
              >
                {group}
              </TableCell>
            </TableRow>
            {rows.map((cap) => (
              <TableRow key={cap.id}>
                <TableCell className="align-top">
                  <div className="text-sm font-medium">{cap.title}</div>
                  <div className="text-muted-foreground text-xs">
                    {cap.description}
                  </div>
                </TableCell>
                {AGENT_COLUMNS.map((agent) => (
                  <TableCell className="align-top" key={agent.id}>
                    <button
                      className="focus:ring-ring cursor-pointer rounded-sm focus:ring-2 focus:outline-none"
                      data-testid={`cap-cell-${cap.id}-${agent.id}`}
                      onClick={() => {
                        onSelect({
                          agentId: agent.id,
                          agentLabel: agent.label,
                          cap,
                          support: cap.perAgent[agent.id],
                        });
                      }}
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
    {LEVELS.map((level) => (
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
              <div className="text-sm font-medium">Note</div>
              <p
                className="text-muted-foreground mt-1 text-sm"
                data-testid="cap-cell-note"
              >
                {cellNote(selection.support)}
              </p>
            </div>
            <div>
              <div className="text-sm font-medium">Legend</div>
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
  const retry = useAtomRefresh(capabilitiesAtom);
  const [selection, setSelection] = useState<Selection | null>(null);

  return (
    <div>
      <ResultView onRetry={retry} result={result}>
        {(caps) => (
          <>
            <Matrix caps={caps} onSelect={setSelection} />
            <Legend />
            <CellSheet
              onClose={() => {
                setSelection(null);
              }}
              selection={selection}
            />
          </>
        )}
      </ResultView>
    </div>
  );
};
