/** The Retries tab: named causes, what each cost in round trips, and its fix.
 *
 * Ranked by sessions with a retry, then sessions, then pairs — the order core
 * applied. Hits, retries, seconds and tokens stay separate columns: none is
 * folded into another. A cause whose rule already sits in a memory file and
 * still fires says so, with the env or prompt-level change that would stop it.
 */
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
import type {
  CauseRow,
  CauseTotals,
  StatsText,
} from "@workspace/view-core/api-types.gen";
import {
  STATS_DATA,
  STATS_EMPTY,
  STATS_FIX_IN,
} from "@workspace/view-core/constants.gen";
import { fmtK } from "@workspace/viz/lib/session-format";
import { Fragment, useState } from "react";

import { approxUsd, hms, NO_RULE, n, pct } from "../../lib/stats-format";
import type { PromptFor } from "../../lib/stats-format";
import { CopyButton } from "./copy-button";
import { HeadTip } from "./head-tip";
import { PromptButton } from "./prompt-button";
import { SessionLinks } from "./session-links";

const { columns } = STATS_DATA;

/** A numeric column a phone drops; `NarrowSummary` says the same in a line. */
const WIDE = "hidden text-right md:table-cell";

/** The rule check of a cause whose rule a memory file already states:
 *  green once the rule stopped it, amber while it still fires. */
const RuleBadge = ({
  fixed,
  line,
}: {
  readonly fixed: boolean;
  readonly line: string;
}) => (
  <Badge
    className={cn(
      "h-auto w-auto justify-start rounded-md text-left whitespace-normal",
      fixed
        ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
        : "border-amber-500/40 bg-amber-500/10 text-amber-400"
    )}
    data-fixed={fixed}
    variant="outline"
  >
    {line}
  </Badge>
);

/** The rule column: the rule check, or a dash when no memory file states it. */
const RuleCell = ({
  fixed,
  line,
}: {
  readonly fixed: boolean;
  readonly line: string | undefined;
}) =>
  line === undefined ? (
    <span className="text-muted-foreground">—</span>
  ) : (
    <RuleBadge fixed={fixed} line={line} />
  );

/** The cause sits in the "working" list: its rule stopped it. */
const isFixed = (text: StatsText, key: string) =>
  text.working.some((w) => w.key === key);

/**
 * The row's columns as one wrapped line, under the cause, for a phone: a
 * seven-column table at 390px shows the first column and nothing else.
 */
const NarrowSummary = ({
  fixed,
  line,
  ruleLine,
}: {
  readonly fixed: boolean;
  readonly line: string | undefined;
  readonly ruleLine: string | undefined;
}) => (
  <div
    className="mt-1 flex flex-col gap-1 md:hidden"
    data-testid="cause-narrow"
  >
    {line === undefined ? null : (
      <p className="text-muted-foreground font-mono text-xs">{line}</p>
    )}
    {ruleLine === undefined ? null : (
      <RuleBadge fixed={fixed} line={ruleLine} />
    )}
  </div>
);

/** `32m14s · 2.8%`: retry time and its share. */
export const timeCell = (sec: number, share: number) =>
  `${hms(sec)} · ${pct(share)}`;

/** `17.4M · ~$9`: re-read tokens and their approximate price. */
export const rereadCell = (tokens: number, usd: number | null) =>
  usd === null ? fmtK(tokens) : `${fmtK(tokens)} · ${approxUsd(usd)}`;

/**
 * The fix, the rule check and the evidence of one cause, opened under it. It
 * lives in the page, not in the drill sheet, so its Copy button stays usable
 * while no modal is open; "All pairs" opens the sheet on top.
 */
const DetailRow = ({
  row,
  onDrill,
  prompt,
  text,
}: {
  readonly onDrill: () => void;
  readonly prompt: PromptFor;
  readonly row: CauseRow;
  readonly text: StatsText;
}) => {
  const rule = text.suggestedRule[row.key] ?? null;

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
            <PromptButton
              subject={row.label}
              testId="cause-prompt"
              text={prompt({ key: row.key, table: "cause" })}
            />
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
            <div
              className={cn(
                "flex flex-col gap-1 rounded-md border p-2 text-xs",
                isFixed(text, row.key)
                  ? "border-emerald-500/40 bg-emerald-500/5"
                  : "border-amber-500/40 bg-amber-500/5"
              )}
            >
              <p>
                {text.ruleLines[row.key]}
                {" — "}
                {row.rule.advice}
              </p>
              {row.rule.matches.map((m) => (
                <code
                  className="text-muted-foreground block wrap-anywhere whitespace-pre-wrap"
                  key={`${m.path}:${m.line}`}
                >
                  {m.path}:{m.line} {m.excerpt}
                </code>
              ))}
            </div>
          )}
          {text.causeSplit[row.key] === undefined ? null : (
            <p
              className="text-muted-foreground font-mono text-xs"
              data-testid="cause-split"
            >
              {text.causeSplit[row.key]}
            </p>
          )}
          {row.diffs.length === 0 ? null : (
            <p className="text-muted-foreground font-mono text-xs">
              learned:{" "}
              {row.diffs
                .map((d) => `${d.from} → ${d.to} (${n(d.pairs)})`)
                .join(" · ")}
            </p>
          )}
          {row.details.length === 0 ? null : (
            <p className="text-muted-foreground font-mono text-xs">
              named:{" "}
              {row.details.map((d) => `${d.value} ${n(d.hits)}`).join(" · ")}
            </p>
          )}
          {row.pairSamples.length === 0 ? null : (
            <div className="flex flex-col gap-0.5" data-testid="cause-pairs">
              {row.pairSamples.map((p) => (
                <code
                  className="text-muted-foreground block text-xs wrap-anywhere whitespace-pre-wrap"
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
/** Every cause added up, under the last row. */
const TotalsRow = ({ totals }: { readonly totals: CauseTotals }) => (
  <TableRow
    className="bg-muted/20 hover:bg-muted/20 font-medium"
    data-testid="cause-totals"
  >
    <TableCell className="text-sm">{STATS_DATA.totals}</TableCell>
    <TableCell className={cn(WIDE, "font-mono text-sm")}>
      {n(totals.hits)}
    </TableCell>
    <TableCell className={cn(WIDE, "font-mono text-sm")}>
      {n(totals.pairs)}
    </TableCell>
    <TableCell className="hidden lg:table-cell" />
    <TableCell className={cn(WIDE, "font-mono text-sm")}>
      {timeCell(totals.retrySec, totals.retryShare)}
    </TableCell>
    <TableCell className="hidden text-right font-mono text-sm lg:table-cell">
      {rereadCell(totals.retryCtxTokens, totals.retryUsd)}
    </TableCell>
    <TableCell className="hidden md:table-cell" />
  </TableRow>
);

export const CauseTable = ({
  debug,
  rows,
  onSelect,
  prompt,
  selectedKey,
  text,
  totals,
}: {
  readonly debug: boolean;
  readonly onSelect: (key: string) => void;
  readonly prompt: PromptFor;
  readonly rows: readonly CauseRow[];
  readonly selectedKey: string | null;
  readonly text: StatsText;
  readonly totals: CauseTotals;
}) => {
  const [openKey, setOpenKey] = useState<string | null>(selectedKey);

  const toggle = (key: string) => {
    setOpenKey((open) => (open === key ? null : key));
  };

  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="cause-empty">
        {STATS_EMPTY.causes}
      </p>
    );
  }

  return (
    <div className="border-border rounded-lg border" data-testid="cause-table">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Cause</TableHead>
            <TableHead className={WIDE}>
              <HeadTip label="Hits" tip={columns.hits} />
            </TableHead>
            <TableHead className={WIDE}>
              <HeadTip label="Retries" tip={columns.retries} />
            </TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              <HeadTip
                label={`${STATS_DATA.you} / ${STATS_DATA.subagents}`}
                tip={columns.split}
              />
            </TableHead>
            <TableHead className={WIDE}>
              <HeadTip label="Retry time" tip={columns.retryTime} />
            </TableHead>
            <TableHead className="hidden text-right lg:table-cell">
              <HeadTip label="Re-read" tip={columns.reread} />
            </TableHead>
            <TableHead className="hidden md:table-cell">
              <HeadTip label="Rule" tip={columns.rule} />
            </TableHead>
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
                onClick={() => {
                  toggle(row.key);
                }}
              >
                <TableCell className="align-top whitespace-normal md:max-w-[42ch] md:whitespace-nowrap">
                  {/* The row is clickable for a mouse; this button is the
                      keyboard and screen-reader route to the same detail. */}
                  <button
                    aria-controls={`cause-detail-${row.key}`}
                    aria-expanded={openKey === row.key}
                    className="focus-visible:ring-ring block max-w-full rounded-sm text-left text-sm font-medium outline-none focus-visible:ring-2 md:truncate"
                    data-testid="cause-toggle"
                    onClick={(event) => {
                      event.stopPropagation();
                      toggle(row.key);
                    }}
                    type="button"
                  >
                    {row.label}
                  </button>
                  <div className="mt-1 flex flex-wrap gap-1">
                    {STATS_FIX_IN[row.kind] === null ? null : (
                      <Badge variant="secondary">
                        {STATS_FIX_IN[row.kind]}
                      </Badge>
                    )}
                    {debug ? (
                      <Badge className="font-mono" variant="outline">
                        {row.key.replace("cause:", "")} · {row.kind}
                      </Badge>
                    ) : null}
                  </div>
                  <NarrowSummary
                    fixed={isFixed(text, row.key)}
                    line={text.causeNarrow[row.key]}
                    ruleLine={text.ruleLines[row.key]}
                  />
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
                <TableCell className="text-muted-foreground hidden text-right align-top font-mono text-sm lg:table-cell">
                  {n(row.hitsBy.main)} / {n(row.hitsBy.nested)}
                </TableCell>
                <TableCell
                  className={cn(
                    WIDE,
                    "text-muted-foreground align-top font-mono text-sm"
                  )}
                >
                  {row.secMeasured === 0
                    ? "—"
                    : timeCell(row.retrySec, row.retryShare)}
                </TableCell>
                <TableCell className="text-muted-foreground hidden text-right align-top font-mono text-sm lg:table-cell">
                  {row.tokensMeasured === 0
                    ? "—"
                    : rereadCell(row.retryCtxTokens, row.retryUsd)}
                </TableCell>
                <TableCell className="hidden min-w-[16ch] align-top text-xs whitespace-normal md:table-cell">
                  <RuleCell
                    fixed={isFixed(text, row.key)}
                    line={text.ruleLines[row.key]}
                  />
                </TableCell>
              </TableRow>
              {openKey === row.key ? (
                <DetailRow
                  onDrill={() => {
                    onSelect(row.key);
                  }}
                  prompt={prompt}
                  row={row}
                  text={text}
                />
              ) : null}
            </Fragment>
          ))}
          {rows.length > 1 ? <TotalsRow totals={totals} /> : null}
        </TableBody>
      </Table>
    </div>
  );
};
