/** What a cold start leads with: the verdict, where the spend goes, then the
 * causes a fix would still save.
 *
 * Not a score. Each card states the measurement in its own unit, the one line
 * that says what to change, where the fix lives, and the sessions it came
 * from. Detector ids show only when the reader asks for them. Dollars, failures
 * and seconds are never added together. A cause whose rule no memory file
 * states yet carries a copy button for that rule line; a cause its rule
 * already stopped sits in the collapsed "working" list instead.
 */
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import { cn } from "@workspace/ui/lib/utils";
import type { DrillTable, Finding } from "@workspace/view-core/api-types.gen";
import {
  STATS_EMPTY,
  STATS_FINDINGS,
  STATS_SUGGESTED,
} from "@workspace/view-core/constants.gen";
import { ListTreeIcon } from "lucide-react";

import type { PromptFor } from "../../lib/stats-format";
import { CopyButton } from "./copy-button";
import { PromptButton } from "./prompt-button";
import { SessionLinks } from "./session-links";

/** Where a finding came from, in one word the reader can trust. */
const SOURCE_LABEL: Record<DrillTable, string> = {
  cause: "retries",
  cluster: "clusters",
  fail: "fails",
  time: "time",
  wait: "waiting",
};

/** A live finding, or a cause its rule already stopped. */
type Look = "live" | "fixed";

/** The prompt and drill of a finding that came from a table row. */
const DrillActions = ({
  finding,
  onDrill,
  prompt,
  table,
}: {
  readonly finding: Finding;
  readonly onDrill: (table: DrillTable, key: string) => void;
  readonly prompt: PromptFor;
  readonly table: DrillTable;
}) => (
  <>
    <PromptButton
      subject={finding.title}
      testId="finding-prompt"
      text={prompt({ key: finding.key, table })}
    />
    <Button
      className="ml-auto h-6 shrink-0 gap-1 px-2 text-xs"
      onClick={() => {
        onDrill(table, finding.key);
      }}
      size="sm"
      variant="ghost"
    >
      <ListTreeIcon className="size-3" />
      All calls
    </Button>
  </>
);

const FindingCard = ({
  debug,
  finding,
  look,
  onDrill,
  prompt,
}: {
  readonly debug: boolean;
  readonly finding: Finding;
  readonly look: Look;
  readonly onDrill: (table: DrillTable, key: string) => void;
  readonly prompt: PromptFor;
}) => (
  <article
    className={cn(
      "border-border flex min-w-0 flex-col gap-2 overflow-hidden rounded-lg border p-3",
      look === "fixed" && "border-emerald-500/40 bg-emerald-500/5",
      look === "live" &&
        finding.detector !== null &&
        "border-amber-500/40 bg-amber-500/5"
    )}
    data-testid={look === "fixed" ? "stats-working-finding" : "stats-finding"}
  >
    <div className="flex flex-wrap items-baseline gap-2">
      <span className="min-w-0 text-sm font-medium wrap-break-word">
        {finding.title}
      </span>
      {finding.fixIn === null ? null : (
        <Badge
          className="shrink-0"
          data-testid="stats-finding-fix-in"
          variant="secondary"
        >
          {finding.fixIn}
        </Badge>
      )}
      {look === "fixed" ? (
        <Badge
          className="shrink-0 border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
          variant="outline"
        >
          fixed
        </Badge>
      ) : null}
      {debug ? (
        <Badge className="shrink-0 font-mono" variant="outline">
          {finding.table === null ? "cost" : SOURCE_LABEL[finding.table]}
          {finding.detector === null ? "" : ` · ${finding.detector}`}
        </Badge>
      ) : null}
      {/* Not shrink-0: a retry impact names four numbers and must wrap, not
          run past the card's clipped edge. */}
      <span
        className="ml-auto min-w-0 font-mono text-sm wrap-break-word"
        data-testid="stats-finding-impact"
      >
        {finding.impact}
      </span>
    </div>
    <p className="text-muted-foreground text-xs">{finding.note}</p>
    <p className="text-muted-foreground font-mono text-xs wrap-break-word">
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
      {finding.table === null ? null : (
        <DrillActions
          finding={finding}
          onDrill={onDrill}
          prompt={prompt}
          table={finding.table}
        />
      )}
    </div>
  </article>
);

/** Causes a rule already stopped, collapsed under one line. */
const Working = ({
  debug,
  findings,
  onDrill,
  prompt,
}: {
  readonly debug: boolean;
  readonly findings: readonly Finding[];
  readonly onDrill: (table: DrillTable, key: string) => void;
  readonly prompt: PromptFor;
}) =>
  findings.length === 0 ? null : (
    <Collapsible data-testid="stats-working">
      <CollapsibleTrigger className="text-muted-foreground text-left text-xs underline decoration-dotted">
        {STATS_FINDINGS.workingTitle} ({findings.length}):{" "}
        {STATS_FINDINGS.workingNote}
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="mt-2 grid gap-3 lg:grid-cols-2">
          {findings.map((finding) => (
            <FindingCard
              debug={debug}
              finding={finding}
              key={finding.key}
              look="fixed"
              onDrill={onDrill}
              prompt={prompt}
            />
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );

/** Every suggested rule line, one copy away. */
const SuggestedRules = ({ rules }: { readonly rules: readonly string[] }) =>
  rules.length === 0 ? null : (
    <div
      className="border-border flex flex-col gap-2 rounded-lg border p-3"
      data-testid="stats-suggested-rules"
    >
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="text-sm font-medium">{STATS_SUGGESTED.title}</span>
        <span className="text-muted-foreground text-xs">
          {STATS_SUGGESTED.note}
        </span>
        <span className="ml-auto">
          <CopyButton
            label="Copy all suggested rules"
            testId="stats-copy-all"
            text={rules.map((r) => `- ${r}`).join("\n")}
          />
        </span>
      </div>
      <ul className="text-muted-foreground list-disc pl-5 text-xs">
        {rules.map((r) => (
          <li key={r}>{r}</li>
        ))}
      </ul>
    </div>
  );

/** The flagged findings band. Empty is a real answer, and says so. */
export const Findings = ({
  debug,
  findings,
  onDrill,
  showAll,
  onShowAll,
  prompt,
  suggested,
  working,
}: {
  readonly debug: boolean;
  readonly findings: readonly Finding[];
  readonly onDrill: (table: DrillTable, key: string) => void;
  readonly onShowAll: () => void;
  readonly prompt: PromptFor;
  readonly showAll: boolean;
  readonly suggested: readonly string[];
  readonly working: readonly Finding[];
}) => {
  if (findings.length === 0) {
    return (
      <div className="flex flex-col gap-3">
        <p
          className="text-muted-foreground text-sm"
          data-testid="stats-no-findings"
        >
          {STATS_EMPTY.findings}
        </p>
        <Working
          debug={debug}
          findings={working}
          onDrill={onDrill}
          prompt={prompt}
        />
      </div>
    );
  }

  const { visible } = STATS_FINDINGS;
  const shown = showAll ? findings : findings.slice(0, visible);

  return (
    <div className="flex flex-col gap-3" data-testid="stats-findings">
      <div className="grid gap-3 lg:grid-cols-2">
        {shown.map((finding) => (
          <FindingCard
            debug={debug}
            finding={finding}
            key={`${finding.table ?? "cost"}:${finding.key}`}
            look="live"
            onDrill={onDrill}
            prompt={prompt}
          />
        ))}
      </div>
      {findings.length > visible && !showAll ? (
        <Button
          className="self-start"
          onClick={onShowAll}
          size="sm"
          variant="outline"
        >
          Show {findings.length - visible} more
        </Button>
      ) : null}
      <Working
        debug={debug}
        findings={working}
        onDrill={onDrill}
        prompt={prompt}
      />
      <SuggestedRules rules={suggested} />
    </div>
  );
};

/** The line the page opens with, and what its total holds that no fix
 * removes. */
export const Verdict = ({
  line,
  note,
}: {
  readonly line: string | null;
  readonly note: string | null;
}) =>
  line === null ? null : (
    <div className="flex flex-col gap-1" data-testid="stats-verdict">
      <p className="text-base font-medium text-balance">{line}</p>
      {note === null ? null : (
        <p className="text-muted-foreground text-xs">{note}</p>
      )}
    </div>
  );
