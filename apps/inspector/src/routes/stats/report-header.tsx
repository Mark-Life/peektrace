/** What the report covers, and the knobs that change it.
 *
 * The corpus line is first because every share below divides by a number in it.
 * The caveats ship with the numbers and are one click away, never dropped: the
 * durations include approval waits, and a silent-failure count is a floor.
 */
import { AGENT_IDS, type AgentId } from "@workspace/core/services/agent-id";
import type { StatsReport } from "@workspace/core/services/stats/schema";
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
import { RefreshCwIcon, ShieldCheckIcon } from "lucide-react";
import { at, hms, n } from "../../lib/stats-format";

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

/** The agent filter value that means "every agent with a parser". */
const ALL_AGENTS = "all";

/** Read the window out of a hash param, falling back to the service default. */
export const daysOf = (raw: string | null): number => {
  const parsed = Number(raw);
  return DAY_OPTIONS.includes(parsed as (typeof DAY_OPTIONS)[number])
    ? parsed
    : DEFAULT_DAYS;
};

/** Read the agent filter out of a hash param. */
export const agentOf = (raw: string | null): AgentId | null =>
  AGENT_IDS.includes(raw as AgentId) ? (raw as AgentId) : null;

/** Window, agent and rescan. All three ride the hash, so a view is a link. */
const Controls = ({
  agent,
  days,
  onAgent,
  onDays,
  onRescan,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly onAgent: (value: AgentId | null) => void;
  readonly onDays: (value: number) => void;
  readonly onRescan: () => void;
}) => (
  <div className="flex flex-wrap items-center gap-2">
    <Select onValueChange={(v) => onDays(Number(v))} value={String(days)}>
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
      onValueChange={(v) => onAgent(v === ALL_AGENTS ? null : (v as AgentId))}
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
      onClick={onRescan}
      size="sm"
      variant="outline"
    >
      <RefreshCwIcon className="size-3.5" />
      Rescan
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
  report,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly onAgent: (value: AgentId | null) => void;
  readonly onDays: (value: number) => void;
  readonly onRescan: () => void;
  readonly report: StatsReport;
}) => {
  const { corpus } = report;
  return (
    <header className="flex flex-col gap-3" data-testid="stats-header">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="font-mono text-sm" data-testid="stats-corpus">
          {n(corpus.transcripts)} transcripts · {n(corpus.projects)} projects ·{" "}
          {n(corpus.toolCalls)} tool calls · {n(corpus.bashCalls)} shell calls ·{" "}
          {hms(corpus.bashSec)} of shell time
        </p>
        <Controls
          agent={agent}
          days={days}
          onAgent={onAgent}
          onDays={onDays}
          onRescan={onRescan}
        />
      </div>
      <p className="text-muted-foreground text-xs">
        {report.window.fromIso.slice(0, 10)} →{" "}
        {report.window.toIso.slice(0, 10)} · generated {at(report.generatedAt)}{" "}
        · {report.schemaVersion} · detector v{report.detectorVersion}
      </p>
      <div className="flex items-center gap-1.5 text-muted-foreground text-xs">
        <ShieldCheckIcon className="size-3.5 shrink-0" />
        Command text, error signatures and cluster labels are redacted before
        they are stored or shown.
      </div>
      <Collapsible>
        <CollapsibleTrigger className="text-left text-muted-foreground text-xs underline decoration-dotted">
          {n(report.caveats.length)} caveats travel with these numbers
          {report.skipped.length > 0
            ? ` · ${n(report.skipped.length)} files skipped`
            : ""}
        </CollapsibleTrigger>
        <CollapsibleContent>
          <ul
            className="mt-2 flex list-disc flex-col gap-1 pl-4 text-muted-foreground text-xs"
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
