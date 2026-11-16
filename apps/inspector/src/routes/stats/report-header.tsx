/** What the report covers, and the knobs that change it.
 *
 * The corpus line is first because every share below divides by a number in it.
 * The caveats ship with the numbers and are one click away, never dropped: the
 * durations include approval waits, and a silent-failure count is a floor.
 */
import { Button } from "@workspace/ui/components/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { cn } from "@workspace/ui/lib/utils";
import type { AgentId, StatsReport } from "@workspace/view-core/api-types.gen";
import { AGENT_IDS, STATS_FINDINGS } from "@workspace/view-core/constants.gen";
import { RefreshCwIcon, ShieldCheckIcon } from "lucide-react";

import { at, hms, n } from "../../lib/stats-format";
import { PromptButton } from "./prompt-button";

const WEEK_DAYS = 7;
const MONTH_DAYS = 30;

/** The window the service uses when the caller names none. */
const DEFAULT_DAYS = 60;
const QUARTER_DAYS = 90;

/** Windows offered, shortest first. */
const DAY_OPTIONS = [
  WEEK_DAYS,
  MONTH_DAYS,
  DEFAULT_DAYS,
  QUARTER_DAYS,
] as const;

/** Length of the `YYYY-MM-DD` prefix of an ISO stamp. */
const ISO_DATE_LENGTH = 10;

/** The agent filter value that means "every agent with a parser". */
const ALL_AGENTS = "all";

/** Read the window out of a hash param, falling back to the service default. */
export const daysOf = (raw: string | null): number => {
  const parsed = Number(raw);

  return DAY_OPTIONS.some((option) => option === parsed)
    ? parsed
    : DEFAULT_DAYS;
};

const isAgentId = (value: string | null): value is AgentId =>
  AGENT_IDS.some((id) => id === value);

/** Read the agent filter out of a hash param. */
export const agentOf = (raw: string | null): AgentId | null =>
  isAgentId(raw) ? raw : null;

/** Window, agent and rescan. All three ride the hash, so a view is a link. */
const Controls = ({
  agent,
  days,
  onAgent,
  onDays,
  onRescan,
  rescanning,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly onAgent: (value: AgentId | null) => void;
  readonly onDays: (value: number) => void;
  readonly onRescan: () => void;
  readonly rescanning: boolean;
}) => (
  <div className="flex flex-wrap items-center gap-2">
    <Select
      onValueChange={(v) => {
        onDays(Number(v));
      }}
      value={String(days)}
    >
      <SelectTrigger className="w-32" data-testid="stats-days">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {DAY_OPTIONS.map((option) => (
          <SelectItem key={option} value={String(option)}>
            {option} days
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
    <Select
      onValueChange={(v) => {
        onAgent(isAgentId(v) ? v : null);
      }}
      value={agent ?? ALL_AGENTS}
    >
      <SelectTrigger className="w-36" data-testid="stats-agent">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL_AGENTS}>All agents</SelectItem>
        {AGENT_IDS.map((id) => (
          <SelectItem key={id} value={id}>
            {id}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
    <Button
      data-testid="stats-rescan"
      disabled={rescanning}
      onClick={onRescan}
      size="sm"
      variant="outline"
    >
      <RefreshCwIcon
        aria-hidden
        className={cn("size-3.5", rescanning && "motion-safe:animate-spin")}
      />
      {rescanning ? "Rescanning…" : "Rescan"}
    </Button>
  </div>
);

/** The corpus every share below divides by, plus the caveats it carries. */
export const ReportHeader = ({
  agent,
  days,
  onAgent,
  onDays,
  onRescan,
  prompt,
  report,
  rescanning,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly onAgent: (value: AgentId | null) => void;
  readonly onDays: (value: number) => void;
  readonly onRescan: () => void;
  /** A rescan is in flight over the report on screen. */
  readonly rescanning: boolean;
  /** The whole-report investigation prompt. */
  readonly prompt: string | null;
  readonly report: StatsReport;
}) => {
  const { corpus } = report;
  const nestedRuns = corpus.transcripts - (corpus.kinds.main ?? 0);

  return (
    <header className="flex flex-col gap-3" data-testid="stats-header">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="font-mono text-sm" data-testid="stats-corpus">
          {n(corpus.sessions)} sessions + {n(nestedRuns)} subagent runs ·{" "}
          {n(corpus.projects)} projects · {n(corpus.toolCalls)} tool calls ·{" "}
          {n(corpus.bashCalls)} shell calls · {hms(corpus.bashSec)} of shell
          time
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Controls
            agent={agent}
            days={days}
            onAgent={onAgent}
            onDays={onDays}
            onRescan={onRescan}
            rescanning={rescanning}
          />
          <PromptButton
            subject="this report"
            testId="stats-prompt"
            text={prompt}
          />
        </div>
      </div>
      <p className="text-muted-foreground text-xs">
        {report.window.fromIso.slice(0, ISO_DATE_LENGTH)} →{" "}
        {report.window.toIso.slice(0, ISO_DATE_LENGTH)} · generated{" "}
        {at(report.generatedAt)} · {report.schemaVersion} · detector v
        {report.detectorVersion}
      </p>
      <div className="text-muted-foreground flex items-center gap-1.5 text-xs">
        <ShieldCheckIcon className="size-3.5 shrink-0" />
        Secrets such as API keys and tokens are masked. Commands and paths are
        shown as run.
      </div>
      <Collapsible>
        <CollapsibleTrigger className="text-muted-foreground text-left text-xs underline decoration-dotted">
          {STATS_FINDINGS.measuredTitle} ({n(report.caveats.length)}{" "}
          {report.caveats.length === 1 ? "note" : "notes"})
          {report.skipped.length > 0
            ? ` · ${n(report.skipped.length)} files skipped`
            : ""}
        </CollapsibleTrigger>
        <CollapsibleContent>
          <ul
            className="text-muted-foreground mt-2 flex list-disc flex-col gap-1 pl-4 text-xs"
            data-testid="stats-caveats"
          >
            {report.caveats.map((caveat) => (
              <li key={caveat}>{caveat}</li>
            ))}
            {report.skipped.map((path) => (
              <li key={path}>skipped: {path}</li>
            ))}
          </ul>
        </CollapsibleContent>
      </Collapsible>
    </header>
  );
};
