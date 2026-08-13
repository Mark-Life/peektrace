/** WHAT FAILS: one ranked table, no severity and no verdict word.
 *
 * Ranked by sessions spanned, then rows — the ranking core applied, redrawn in
 * that order. `flagged` is printed beside `calls` because the gap between them
 * is the point: a call that printed a failure and exited 0 never set the flag.
 * A row says "printed a failure", never "this broke".
 */
import type { FailRow } from "@workspace/core/services/stats/schema";
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
import { n } from "../../lib/stats-format";
import { SessionLinks } from "./session-links";

/** The rows behind one key, opened in place under it. */
const DetailRow = ({ row }: { readonly row: FailRow }) => (
  <TableRow className="bg-muted/30 hover:bg-muted/30">
    <TableCell colSpan={6}>
      <div className="flex flex-col gap-2 py-1" data-testid="fail-detail">
        {row.note ? <p className="text-xs">{row.note}</p> : null}
        <SessionLinks detector={row.detector} samples={row.samples} />
      </div>
    </TableCell>
  </TableRow>
);

/** The fail table. Clicking a row opens the calls behind it. */
export const FailTable = ({
  rows,
  onSelect,
  selectedKey,
}: {
  readonly onSelect: (key: string) => void;
  readonly rows: readonly FailRow[];
  readonly selectedKey: string | null;
}) => {
  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="fail-empty">
        Nothing failed in this window.
      </p>
    );
  }
  return (
    <div className="rounded-lg border border-border" data-testid="fail-table">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>What</TableHead>
            <TableHead>Where</TableHead>
            <TableHead className="text-right">Sessions</TableHead>
            <TableHead className="text-right">Calls</TableHead>
            <TableHead className="text-right">Flagged</TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              Projects
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
                data-testid="fail-row"
                onClick={() => onSelect(row.key)}
              >
                <TableCell className="max-w-[42ch] align-top">
                  <div className="truncate font-medium text-sm">
                    {row.label}
                  </div>
                  {row.detector === null ? null : (
                    <Badge className="mt-1 font-mono" variant="outline">
                      {row.detector}
                    </Badge>
                  )}
                </TableCell>
                <TableCell className="align-top text-muted-foreground text-xs">
                  {row.where}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-sm">
                  {n(row.sessions)}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-sm">
                  {n(row.rows)}
                </TableCell>
                <TableCell className="text-right align-top font-mono text-muted-foreground text-sm">
                  {n(row.flagged)}
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm lg:table-cell">
                  {n(row.projects)}
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
