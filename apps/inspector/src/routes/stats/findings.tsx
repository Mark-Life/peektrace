/** What a cold start leads with: the rows that carry a fix.
 *
 * Not a score. Each card states the measurement in its own table's unit, the one
 * line that says what to change, and the sessions it came from. Failures and
 * seconds are never added together — the cards are ordered by how cheap the fix
 * is, then by the impact their own table already ranked them on. A cause whose
 * rule no memory file states yet carries a copy button for that rule line.
 */
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import { cn } from "@workspace/ui/lib/utils";
import { ListTreeIcon } from "lucide-react";
import type { Finding } from "../../lib/stats-format";
import { CopyButton } from "./copy-button";
import { SessionLinks } from "./session-links";

/** Cards shown before the reader asks for more. */
const VISIBLE = 6;

/** Where a finding came from, in one word the reader can trust. */
const SOURCE_LABEL: Record<Finding["table"], string> = {
  cause: "retries",
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
      "flex min-w-0 flex-col gap-2 overflow-hidden rounded-lg border border-border p-3",
      finding.detector !== null && "border-amber-500/40 bg-amber-500/5"
    )}
    data-testid="stats-finding"
  >
    <div className="flex flex-wrap items-baseline gap-2">
      <span className="wrap-break-word min-w-0 font-medium text-sm">
        {finding.title}
      </span>
      <Badge className="shrink-0" variant="secondary">
        {SOURCE_LABEL[finding.table]}
      </Badge>
      {finding.detector === null ? null : (
        <Badge className="shrink-0 font-mono" variant="outline">
          {finding.detector}
        </Badge>
      )}
      {/* Not shrink-0: a retry impact names four numbers and must wrap, not
          run past the card's clipped edge. */}
      <span
        className="wrap-break-word ml-auto min-w-0 font-mono text-sm"
        data-testid="stats-finding-impact"
      >
        {finding.impact}
      </span>
    </div>
    <p className="text-muted-foreground text-xs">{finding.note}</p>
    <p className="wrap-break-word font-mono text-muted-foreground text-xs">
      {finding.spread}
    </p>
    <div className="flex min-w-0 flex-wrap items-end justify-between gap-2">
      <SessionLinks detector={finding.detector} samples={finding.samples} />
      {finding.rule === null ? null : (
        <CopyButton
          label="Copy rule"
          testId="finding-copy"
          text={finding.rule}
        />
      )}
      <Button
        className="ml-auto h-6 shrink-0 gap-1 px-2 text-xs"
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

/** Every suggested rule line, one copy away. */
const SuggestedRules = ({ rules }: { readonly rules: readonly string[] }) =>
  rules.length === 0 ? null : (
    <div
      className="flex flex-col gap-2 rounded-lg border border-border p-3"
      data-testid="stats-suggested-rules"
    >
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="font-medium text-sm">Suggested rules</span>
        <span className="text-muted-foreground text-xs">
          lines no CLAUDE.md / AGENTS.md states yet
        </span>
        <span className="ml-auto">
          <CopyButton
            label="Copy all suggested rules"
            testId="stats-copy-all"
            text={rules.map((r) => `- ${r}`).join("\n")}
          />
        </span>
      </div>
      <ul className="list-disc pl-5 text-muted-foreground text-xs">
        {rules.map((r) => (
          <li key={r}>{r}</li>
        ))}
      </ul>
    </div>
  );

/** The flagged findings band. Empty is a real answer, and says so. */
export const Findings = ({
  findings,
  onDrill,
  showAll,
  onShowAll,
  suggested,
}: {
  readonly findings: readonly Finding[];
  readonly onDrill: (table: Finding["table"], key: string) => void;
  readonly onShowAll: () => void;
  readonly showAll: boolean;
  readonly suggested: readonly string[];
}) => {
  if (findings.length === 0) {
    return (
      <p
        className="text-muted-foreground text-sm"
        data-testid="stats-no-findings"
      >
        No row in this window carries a named fix. The tables below still rank
        what happened.
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
      <SuggestedRules rules={suggested} />
    </div>
  );
};
