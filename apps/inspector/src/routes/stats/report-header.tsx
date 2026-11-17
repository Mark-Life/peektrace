/** The knobs that change the report at the top; what it covers at the foot.
 *
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
import { Switch } from "@workspace/ui/components/switch";
import { cn } from "@workspace/ui/lib/utils";
import type { AgentId, StatsReport } from "@workspace/view-core/api-types.gen";
import { AGENT_IDS, STATS_DATA } from "@workspace/view-core/constants.gen";
import { RefreshCwIcon, ShieldCheckIcon } from "lucide-react";

import { at } from "../../lib/stats-format";
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

/** The knobs that change the report, and the prompt for all of it. */
export const ReportHeader = ({
  agent,
  days,
  onAgent,
  onDays,
  onRescan,
  prompt,
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
}) => (
  <header
    className="flex flex-wrap items-center gap-2"
    data-testid="stats-header"
  >
    <Controls
      agent={agent}
      days={days}
      onAgent={onAgent}
      onDays={onDays}
      onRescan={onRescan}
      rescanning={rescanning}
    />
    <PromptButton subject="this report" testId="stats-prompt" text={prompt} />
  </header>
);

/** What the report covers, how it was measured, and the switch that shows
 * detector ids. Every share above divides by a number in the corpus line. */
export const ReportFooter = ({
  debug,
  onDebug,
  report,
}: {
  readonly debug: boolean;
  readonly onDebug: (on: boolean) => void;
  readonly report: StatsReport;
}) => (
  <footer
    className="border-border text-muted-foreground flex flex-col gap-2 border-t pt-4 text-xs"
    data-testid="stats-footer"
  >
    <p className="font-mono" data-testid="stats-corpus">
      {report.text.corpusLine}
    </p>
    <p>
      {report.window.fromIso.slice(0, ISO_DATE_LENGTH)} →{" "}
      {report.window.toIso.slice(0, ISO_DATE_LENGTH)} · generated{" "}
      {at(report.generatedAt)} · {report.schemaVersion} · detector v
      {report.detectorVersion}
    </p>
    <div className="flex items-center gap-1.5">
      <ShieldCheckIcon className="size-3.5 shrink-0" />
      {STATS_DATA.redacted}
    </div>
    <Collapsible>
      <CollapsibleTrigger className="text-left underline decoration-dotted">
        {report.text.measuredLink}
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ul
          className="mt-2 flex list-disc flex-col gap-1 pl-4"
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
    <label className="flex items-center gap-2 self-start">
      <Switch
        checked={debug}
        data-testid="stats-debug"
        onCheckedChange={onDebug}
      />
      {STATS_DATA.debug}
    </label>
  </footer>
);
