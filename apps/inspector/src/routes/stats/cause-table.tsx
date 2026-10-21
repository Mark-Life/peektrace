/** AVOIDABLE RETRIES: named causes, what each cost in round trips, and its fix.
 *
 * Ranked by sessions with a retry, then sessions, then pairs — the order core
 * applied. Hits, retries, seconds and tokens stay separate columns: none is
 * folded into another. A cause whose rule already sits in a memory file and
 * still fires says so, with the env or prompt-level change that would stop it.
 */
import {
  rateText,
  suggestedRuleOf,
} from "@workspace/core/services/stats/findings";
import type {
  CauseRow,
  RuleCheck,
} from "@workspace/core/services/stats/schema";
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { cn } from "@workspace/ui/lib/utils";
import { fmtK } from "@workspace/viz/lib/session-format";
import { Fragment, useState } from "react";
import { hms, NO_RULE, n, ruleLine } from "../../lib/stats-format";
import { CopyButton } from "./copy-button";
import { SessionLinks } from "./session-links";

/** A numeric column a phone drops; `NarrowSummary` says the same in a line. */
const WIDE = "hidden text-right md:table-cell";

/** The rule check of a cause whose rule a memory file already states. */
const RuleBadge = ({ rule }: { readonly rule: RuleCheck }) => (
  <Badge
    className="h-auto w-auto justify-start whitespace-normal rounded-md border-amber-500/40 bg-amber-500/10 text-left text-amber-400"
    variant="outline"
  >
    {ruleLine(rule)}
  </Badge>
);

/**
 * The row's columns as one wrapped line, under the cause, for a phone: a
 * seven-column table at 390px shows the first column and nothing else.
 */
const NarrowSummary = ({ row }: { readonly row: CauseRow }) => (
  <div
    className="mt-1 flex flex-col gap-1 md:hidden"
    data-testid="cause-narrow"
  >
    <p className="font-mono text-muted-foreground text-xs">
      {n(row.hits)} hits · {n(row.pairs)}{" "}
      {row.failing ? "retries" : "read-backs"} in {n(row.retrySessions)}{" "}
      sessions
      {row.secMeasured === 0 ? "" : ` · ${hms(row.retrySec)}`}
      {row.tokensMeasured === 0
        ? ""
        : ` · ${fmtK(row.retryCtxTokens)} ctx re-read`}
    </p>
    {row.rule === null ? null : <RuleBadge rule={row.rule} />}
  </div>
);

/** Who made the calls and what the retries re-read: the split columns a
 * narrow screen hides, always shown once the row is open. */
const SplitLine = ({ row }: { readonly row: CauseRow }) => (
  <p
    className="font-mono text-muted-foreground text-xs"
    data-testid="cause-split"
  >
    hits main {n(row.hitsBy.main)} / sub {n(row.hitsBy.nested)} · per 1k calls
    in this window main {rateText(row.per1k.main, row.hitsBy.main)} / sub{" "}
    {rateText(row.per1k.nested, row.hitsBy.nested)} ·{" "}
    {row.failing ? "retries" : "read-backs"} main {n(row.pairsBy.main)} / sub{" "}
    {n(row.pairsBy.nested)} · exit 0 {n(row.hits - row.flagged)} · context
    re-read{" "}
    {row.tokensMeasured === 0 ? "not measured" : fmtK(row.retryCtxTokens)}
  </p>
);

/**
 * The fix, the rule check and the evidence of one cause, opened under it. It
 * lives in the page, not in the drill sheet, so its Copy button stays usable
 * while no modal is open; "All pairs" opens the sheet on top.
 */
const DetailRow = ({
  row,
  onDrill,
}: {
  readonly onDrill: () => void;
  readonly row: CauseRow;
}) => {
  const rule = suggestedRuleOf(row);
  return (
    <TableRow
      className="bg-muted/30 hover:bg-muted/30"
      id={`cause-detail-${row.key}`}
    >
      <TableCell className="whitespace-normal" colSpan={7}>
        <div
          className="flex w-0 min-w-full flex-col gap-2 py-1"
          data-testid="cause-detail"
        >
          <div className="flex flex-wrap items-start gap-2">
            <p className="min-w-0 flex-1 text-xs">{row.fix ?? NO_RULE}</p>
            {rule === null ? null : (
              <CopyButton label="Copy rule" testId="cause-copy" text={rule} />
            )}
            <Button
              className="h-6 shrink-0 px-2 text-xs"
              data-testid="cause-drill"
              onClick={onDrill}
              size="sm"
              variant="outline"
            >
              All pairs
            </Button>
          </div>
          {row.rule === null ? null : (
            <div className="flex flex-col gap-1 rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
              <p>
                {ruleLine(row.rule)}
                {" — "}
                {row.rule.advice}
              </p>
              {row.rule.matches.map((m) => (
                <code
                  className="wrap-anywhere block whitespace-pre-wrap text-muted-foreground"
                  key={`${m.path}:${m.line}`}
                >
                  {m.path}:{m.line} {m.excerpt}
                </code>
              ))}
            </div>
          )}
          <SplitLine row={row} />
          {row.diffs.length === 0 ? null : (
            <p className="font-mono text-muted-foreground text-xs">
              learned:{" "}
              {row.diffs
                .map((d) => `${d.from} → ${d.to} (${n(d.pairs)})`)
                .join(" · ")}
            </p>
          )}
          {row.details.length === 0 ? null : (
            <p className="font-mono text-muted-foreground text-xs">
              named:{" "}
              {row.details.map((d) => `${d.value} ${n(d.hits)}`).join(" · ")}
            </p>
          )}
          {row.pairSamples.length === 0 ? null : (
            <div className="flex flex-col gap-0.5" data-testid="cause-pairs">
              {row.pairSamples.map((p) => (
                <code
                  className="wrap-anywhere block whitespace-pre-wrap text-muted-foreground text-xs"
                  key={`${p.failed.sid}#${p.failed.seq}`}
                >
                  {p.failed.line} → {p.retry.line}
                </code>
              ))}
            </div>
          )}
          <SessionLinks detector={row.cause} samples={row.samples} />
        </div>
      </TableCell>
    </TableRow>
  );
};

/**
 * The cause table. Clicking a row opens its detail under it; the detail's
 * "All pairs" opens the drill sheet. A row arrived at through a drill link
 * starts open.
 */
export const CauseTable = ({
  rows,
  onSelect,
  selectedKey,
}: {
  readonly onSelect: (key: string) => void;
  readonly rows: readonly CauseRow[];
  readonly selectedKey: string | null;
}) => {
  const [openKey, setOpenKey] = useState<string | null>(selectedKey);
  const toggle = (key: string) =>
    setOpenKey((open) => (open === key ? null : key));
  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="cause-empty">
        No named cause fired in this window.
      </p>
    );
  }
  return (
    <div className="rounded-lg border border-border" data-testid="cause-table">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Cause</TableHead>
            <TableHead className={WIDE}>Hits</TableHead>
            <TableHead className={WIDE}>Retries</TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              Hits main / sub
            </TableHead>
            <TableHead className={WIDE}>Retry time</TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              Context re-read
            </TableHead>
            <TableHead className="hidden md:table-cell">Rule</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <Fragment key={row.key}>
              <TableRow
                className={cn(
                  "cursor-pointer",
                  (openKey === row.key || selectedKey === row.key) &&
                    "bg-muted/40"
                )}
                data-testid="cause-row"
                onClick={() => toggle(row.key)}
              >
                <TableCell className="whitespace-normal align-top md:max-w-[42ch] md:whitespace-nowrap">
                  {/* The row is clickable for a mouse; this button is the
                      keyboard and screen-reader route to the same detail. */}
                  <button
                    aria-controls={`cause-detail-${row.key}`}
                    aria-expanded={openKey === row.key}
                    className="block max-w-full rounded-sm text-left font-medium text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring md:truncate"
                    data-testid="cause-toggle"
                    onClick={(event) => {
                      event.stopPropagation();
                      toggle(row.key);
                    }}
                    type="button"
                  >
                    {row.label}
                  </button>
                  <div className="mt-1 flex gap-1">
                    <Badge className="font-mono" variant="outline">
                      {row.key.replace("cause:", "")}
                    </Badge>
                    <Badge variant="secondary">{row.kind}</Badge>
                  </div>
                  <NarrowSummary row={row} />
                </TableCell>
                <TableCell className={cn(WIDE, "align-top font-mono text-sm")}>
                  {n(row.hits)}
                </TableCell>
                <TableCell className={cn(WIDE, "align-top font-mono text-sm")}>
                  {n(row.pairs)}
                  <div className="text-muted-foreground text-xs">
                    {n(row.retrySessions)} sessions
                  </div>
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm lg:table-cell">
                  {n(row.hitsBy.main)} / {n(row.hitsBy.nested)}
                </TableCell>
                <TableCell
                  className={cn(
                    WIDE,
                    "align-top font-mono text-muted-foreground text-sm"
                  )}
                >
                  {row.secMeasured === 0 ? "—" : hms(row.retrySec)}
                </TableCell>
                <TableCell className="hidden text-right align-top font-mono text-muted-foreground text-sm lg:table-cell">
                  {row.tokensMeasured === 0 ? "—" : fmtK(row.retryCtxTokens)}
                </TableCell>
                <TableCell className="hidden min-w-[16ch] whitespace-normal align-top text-xs md:table-cell">
                  {row.rule === null ? (
                    <span className="text-muted-foreground">—</span>
                  ) : (
                    <RuleBadge rule={row.rule} />
                  )}
                </TableCell>
              </TableRow>
              {openKey === row.key ? (
                <DetailRow onDrill={() => onSelect(row.key)} row={row} />
              ) : null}
            </Fragment>
          ))}
        </TableBody>
      </Table>
    </div>
  );
};
