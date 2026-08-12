/** What a cold start leads with: the rows that carry a fix.
 *
 * Not a score. Each card states the measurement in its own table's unit, the one
 * line that says what to change, and the sessions it came from. Failures and
 * seconds are never added together — the cards are ordered by how cheap the fix
 * is, then by the impact their own table already ranked them on.
 */
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import { cn } from "@workspace/ui/lib/utils";
import { ListTreeIcon } from "lucide-react";
import type { Finding } from "../../lib/stats-format";
import { SessionLinks } from "./session-links";

/** Cards shown before the reader asks for more. */
const VISIBLE = 6;

/** Where a finding came from, in one word the reader can trust. */
const SOURCE_LABEL: Record<Finding["table"], string> = {
  fail: "fails",
  time: "time",
};

const FindingCard = ({
  finding,
  onDrill,
}: {
  readonly finding: Finding;
  readonly onDrill: (table: Finding["table"], key: string) => void;
}) => (
  <article
    className={cn(
      "flex flex-col gap-2 rounded-lg border border-border p-3",
      finding.detector !== null && "border-amber-500/40 bg-amber-500/5"
    )}
    data-testid="stats-finding"
  >
    <div className="flex flex-wrap items-baseline gap-2">
      <span className="font-medium text-sm">{finding.title}</span>
      <Badge className="shrink-0" variant="secondary">
        {SOURCE_LABEL[finding.table]}
      </Badge>
      {finding.detector === null ? null : (
        <Badge className="shrink-0 font-mono" variant="outline">
          {finding.detector}
        </Badge>
      )}
      <span className="ml-auto shrink-0 font-mono text-sm">
        {finding.impact}
      </span>
    </div>
    <p className="text-muted-foreground text-xs">{finding.note}</p>
    <p className="font-mono text-muted-foreground text-xs">{finding.spread}</p>
    <div className="flex flex-wrap items-end justify-between gap-2">
      <SessionLinks detector={finding.detector} samples={finding.samples} />
      <Button
        className="h-6 gap-1 px-2 text-xs"
        onClick={() => onDrill(finding.table, finding.key)}
        size="sm"
        variant="ghost"
      >
        <ListTreeIcon className="size-3" />
        All calls
      </Button>
    </div>
  </article>
);

/** The flagged findings band. Empty is a real answer, and says so. */
export const Findings = ({
  findings,
  onDrill,
  showAll,
  onShowAll,
}: {
  readonly findings: readonly Finding[];
  readonly onDrill: (table: Finding["table"], key: string) => void;
  readonly onShowAll: () => void;
  readonly showAll: boolean;
}) => {
  if (findings.length === 0) {
    return (
      <p
        className="text-muted-foreground text-sm"
        data-testid="stats-no-findings"
      >
        No row in this window carries a named fix. The two tables below still
        rank what happened.
      </p>
    );
  }
  const shown = showAll ? findings : findings.slice(0, VISIBLE);
  return (
    <div className="flex flex-col gap-3" data-testid="stats-findings">
      <div className="grid gap-3 lg:grid-cols-2">
        {shown.map((finding) => (
          <FindingCard
            finding={finding}
            key={`${finding.table}:${finding.key}`}
            onDrill={onDrill}
          />
        ))}
      </div>
      {findings.length > VISIBLE && !showAll ? (
        <Button
          className="self-start"
          onClick={onShowAll}
          size="sm"
          variant="outline"
        >
          Show {findings.length - VISIBLE} more
        </Button>
      ) : null}
    </div>
  );
};
