/** WHAT COSTS TIME: seconds by job, each command line charged to exactly one.
 *
 * `runs` is the exclusive charge and `co-present` counts the lines that hold the
 * job and were charged to a higher-priority one — printed side by side because
 * counting a line for every job it touches is how a typecheck share doubles.
 * Capped and backgrounded runs are annotations on the row, never their own total.
 * The column is "time", never "time wasted".
 */
import type { TimeRow } from "@workspace/core/services/stats/schema";
import { Badge } from "@workspace/ui/components/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { cn } from "@workspace/ui/lib/utils";
import { Fragment } from "react";
import { coPresentNote, hms, n, pct, timeNote } from "../../lib/stats-format";
import { SessionLinks } from "./session-links";

/** The measured caveats of one job row, opened under it. */
const DetailRow = ({ row }: { readonly row: TimeRow }) => {
  const note = timeNote(row);
  return (
    <TableRow className="bg-muted/30 hover:bg-muted/30">
      <TableCell colSpan={7}>
        <div className="flex flex-col gap-2 py-1" data-testid="time-detail">
          <p className="text-muted-foreground text-xs">{coPresentNote(row)}</p>
          {note ? <p className="text-xs">{note}</p> : null}
          {row.bgRuns > 0 ? (
            <p className="text-muted-foreground text-xs">
              backgrounded: {n(row.bgRuns)} runs, {hms(row.bgSec)} charged
            </p>
          ) : null}
          <SessionLinks detector={null} samples={row.samples} />
        </div>
      </TableCell>
    </TableRow>
  );
};

/** The time table, ranked by seconds. Clicking a row opens its calls. */
export const TimeTable = ({
  rows,
  onSelect,
  selectedKey,
}: {
  readonly onSelect: (key: string) => void;
  readonly rows: readonly TimeRow[];
  readonly selectedKey: string | null;
}) => {
  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="time-empty">
        No shell time measured in this window.
      </p>
    );
  }
  return (
    <div className="rounded-lg border border-border" data-testid="time-table">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Job</TableHead>
            <TableHead className="text-right">Time</TableHead>
            <TableHead className="text-right">Share</TableHead>
            <TableHead className="text-right">Runs</TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              Co-present
            </TableHead>
            <TableHead className="hidden text-right md:table-cell">
              Median
            </TableHead>
            <TableHead className="hidden text-right md:table-cell">
              p95
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <Fragment key={row.key}>
              <TableRow
                className={cn(
                  "cursor-pointer",
                  selectedKey === row.key && "bg-muted/40"
                )}
                data-testid="time-row"
                onClick={() => onSelect(row.key)}
              >
                <TableCell className="align-top">
                  <span className="font-medium text-sm">{row.label}</span>
                  {row.cappedRuns > 0 ? (
                    <Badge className="ml-2" variant="outline">
                      {n(row.cappedRuns)} capped
                    </Badge>
                  ) : null}
                  {row.bgRuns > 0 ? (
                    <Badge className="ml-2" variant="outline">
                      {n(row.bgRuns)} bg
                    </Badge>
                  ) : null}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-sm">
                  {hms(row.totalSec)}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-sm">
                  {pct(row.share)}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-sm">
                  {n(row.runs)}
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm lg:table-cell">
                  {n(row.coPresentRuns)}
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm md:table-cell">
                  {hms(row.medianSec)}
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm md:table-cell">
                  {hms(row.p95Sec)}
                </TableCell>
              </TableRow>
              {selectedKey === row.key ? <DetailRow row={row} /> : null}
            </Fragment>
          ))}
        </TableBody>
      </Table>
    </div>
  );
};
