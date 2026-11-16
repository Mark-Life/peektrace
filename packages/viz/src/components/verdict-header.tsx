/** Verdict header + peak gauge.
 *
 * A health word (Healthy / Degrading / Rotting) derived from peak context vs
 * window, the peak gauge with its dumb-zone marker, and the headline metadata
 * grid (peak, turns, system tax, output, dumb-zone dwell, cache, and the
 * cost and its source when the session has one).
 */
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import type { AnalyzedSession } from "@workspace/view-core/api-types.gen";
import { costSummary } from "@workspace/view-core/cost";
import type { ReactNode } from "react";

import {
  fmt,
  fmtK,
  fmtPct,
  HEALTH_LABEL,
  PERCENT,
  ZONE_CLASSES,
  zoneOf,
} from "../lib/session-format";
import type { SessionJump } from "../lib/session-jump";

/** An analysis without its per-event and per-turn arrays: all the summary cards read. */
export type SessionOverview = Omit<
  AnalyzedSession,
  "events" | "snapshots" | "turns"
>;

/** Short id prefix shown in the metadata line. */
const ID_PREFIX = 8;

/** Display label per transcript provider. */
const PROVIDER_LABEL: ReadonlyMap<string, string> = new Map([
  ["claude-code", "Claude Code"],
  ["codex", "Codex"],
  ["opencode", "OpenCode"],
  ["pi", "Pi"],
]);

/** The session's display name: its title, else the harness that produced it. */
export const analyzedTitle = (a: Pick<AnalyzedSession, "provider" | "title">) =>
  a.title !== undefined && a.title !== ""
    ? a.title
    : `${PROVIDER_LABEL.get(a.provider) ?? a.provider} session`;

/** A single metadata cell. */
const Meta = ({
  k,
  children,
}: {
  readonly k: string;
  readonly children: ReactNode;
}) => (
  <div className="border-border bg-card min-w-0 rounded-md border px-3 py-2">
    <div className="text-muted-foreground truncate text-xs tracking-wide uppercase">
      {k}
    </div>
    <div className="mt-0.5 text-sm wrap-anywhere">{children}</div>
  </div>
);

/**
 * Cost cell value as visible text: total (or `—` when nothing is priced), its
 * source, the notes that qualify it, and a warning when the agent's record
 * and our estimate disagree.
 */
const CostValue = ({
  cost,
  provider,
}: {
  readonly cost: NonNullable<AnalyzedSession["cost"]>;
  readonly provider: string;
}) => {
  const { total, notes, warning } = costSummary(
    cost,
    PROVIDER_LABEL.get(provider) ?? provider
  );

  return (
    <>
      <span
        data-testid="session-cost"
        title={
          cost.source === "estimate" ? "Estimate at list price" : undefined
        }
      >
        {total}
        {notes.length > 0 ? (
          <span className="text-muted-foreground"> · {notes.join(" · ")}</span>
        ) : null}
      </span>
      {warning === undefined ? null : (
        <span
          className="block text-amber-700 dark:text-amber-400"
          data-testid="session-cost-warning"
          title="A model may lack a price card, or the transcript misses calls"
        >
          <span className="sr-only">Warning: </span>
          {warning}
        </span>
      )}
    </>
  );
};

/** A summary chip. Given `onJump` it is a button that shows the place it
 *  names in the transcript; without it, a plain badge. The visible text stays
 *  the button's name, so its counts are read out and voice control matches
 *  it; `hint` follows it for screen readers and as the tooltip. */
const JumpChip = ({
  children,
  className,
  hint,
  jump,
  onJump,
  testId,
}: {
  readonly children: ReactNode;
  readonly className?: string;
  readonly hint: string;
  readonly jump: SessionJump;
  readonly onJump: ((j: SessionJump) => void) | undefined;
  readonly testId: string;
}) =>
  onJump ? (
    <Badge
      asChild
      className={cn(
        "hover:bg-muted min-h-6 cursor-pointer focus-visible:outline-none",
        className
      )}
      variant="outline"
    >
      <button
        data-testid={testId}
        onClick={() => {
          onJump(jump);
        }}
        title={hint}
        type="button"
      >
        {children}
        <span className="sr-only">, {hint}</span>
      </button>
    </Badge>
  ) : (
    <Badge className={className} variant="outline">
      {children}
    </Badge>
  );

/** The peak gauge track: fill width = peak fraction, dumb-zone tick overlaid. */
const PeakGauge = ({ a }: { readonly a: SessionOverview }) => {
  const frac = a.contextWindow ? a.peakContextTokens / a.contextWindow : 0;
  const zone = zoneOf({ dumbZone: a.dumbZoneFraction, frac });

  return (
    <div className="flex flex-col gap-1" data-testid="peak-gauge">
      <div className="bg-muted relative h-4 overflow-hidden rounded-full">
        <div
          className={cn("h-full", ZONE_CLASSES[zone].fill)}
          style={{ width: `${Math.min(PERCENT, frac * PERCENT).toFixed(1)}%` }}
        />
        <div
          className="bg-foreground/70 absolute top-0 h-full w-0.5"
          style={{ left: `${(a.dumbZoneFraction * PERCENT).toFixed(0)}%` }}
          title={`dumb zone ${Math.round(a.dumbZoneFraction * PERCENT)}%`}
        />
      </div>
      <div className="text-muted-foreground flex justify-between text-xs">
        <span data-testid="peak-tokens">peak {fmt(a.peakContextTokens)}</span>
        <span>
          {fmtPct(frac)} of {fmt(a.contextWindow)}
        </span>
      </div>
    </div>
  );
};

/** Verdict header card: title, health badge, gauge, and metadata grid. With
 *  `onJump`, the dumb-zone, compaction and subagent chips jump to the
 *  transcript. */
export const VerdictHeader = ({
  a,
  onJump,
}: {
  readonly a: SessionOverview;
  readonly onJump?: (j: SessionJump) => void;
}) => {
  const frac = a.contextWindow ? a.peakContextTokens / a.contextWindow : 0;
  const zone = zoneOf({ dumbZone: a.dumbZoneFraction, frac });

  const cacheFrac = a.peakContextTokens
    ? a.peakCacheReadTokens / a.peakContextTokens
    : 0;

  return (
    <section
      className="border-border @container flex flex-col gap-4 rounded-lg border p-3 md:p-4"
      data-testid="verdict-header"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1 basis-64">
          <div className="text-lg font-semibold">{analyzedTitle(a)}</div>
          <div
            className="text-muted-foreground truncate font-mono text-xs"
            title={a.cwd ?? undefined}
          >
            {a.sessionId.slice(0, ID_PREFIX)} · {a.models.join(", ")} ·{" "}
            {a.cwd ?? ""}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge data-testid="session-provider" variant="outline">
            {PROVIDER_LABEL.get(a.provider) ?? a.provider}
          </Badge>
          <Badge
            className={cn("text-sm", ZONE_CLASSES[zone].text)}
            data-testid="health-verdict"
            variant="outline"
          >
            {HEALTH_LABEL[zone]}
          </Badge>
        </div>
      </div>

      <PeakGauge a={a} />

      <div className="grid grid-cols-2 gap-2 @lg:grid-cols-3 @4xl:grid-cols-6">
        <Meta k="Peak context">
          <span className={ZONE_CLASSES[zone].text}>
            {fmt(a.peakContextTokens)}
          </span>
        </Meta>
        <Meta k="Turns">
          {a.turnCount}
          <span className="text-muted-foreground">
            {" "}
            · {a.userMessageCount} user · {a.toolCallCount} tools
          </span>
        </Meta>
        <Meta k="System + tools tax">{fmt(a.systemOverheadTokens)}</Meta>
        <Meta k="Output">{fmt(a.totalOutputTokens)}</Meta>
        {a.cost ? (
          <Meta k="Cost">
            <CostValue cost={a.cost} provider={a.provider} />
          </Meta>
        ) : null}
        <Meta k="Dumb-zone dwell">
          {a.dumbZoneTurns}/{a.turnCount}
        </Meta>
        <Meta k="Cache">{fmtPct(cacheFrac)}</Meta>
      </div>

      <div className="flex flex-wrap gap-2">
        <Badge variant="secondary">
          window {fmtK(a.contextWindow)}
          {a.contextWindowInferred ? " (assumed)" : ""}
        </Badge>
        {a.dumbZoneCrossTurn >= 0 ? (
          <JumpChip
            className={ZONE_CLASSES.bad.text}
            hint="jump to that turn in history"
            jump={{ kind: "turn", turn: a.dumbZoneCrossTurn + 1 }}
            onJump={onJump}
            testId="chip-dumbzone"
          >
            entered dumb zone @ turn {a.dumbZoneCrossTurn + 1}
          </JumpChip>
        ) : (
          <Badge className={ZONE_CLASSES.ok.text} variant="outline">
            stayed in smart zone
          </Badge>
        )}
        {a.compactionTurns.length > 0 ? (
          <JumpChip
            hint="jump to the first in history"
            jump={{ kind: "compaction" }}
            onJump={onJump}
            testId="chip-compaction"
          >
            {a.compactionTurns.length} compaction(s)
          </JumpChip>
        ) : null}
        {a.subagents.length > 0 ? (
          <JumpChip
            hint="jump to the first in history"
            jump={{ kind: "subagent" }}
            onJump={onJump}
            testId="chip-subagents"
          >
            {a.subagents.length} subagent(s)
          </JumpChip>
        ) : null}
        {a.webSearchRequests > 0 ? (
          <Badge data-testid="web-searches" variant="outline">
            {a.webSearchRequests} web search(es)
          </Badge>
        ) : null}
        {a.webFetchRequests > 0 ? (
          <Badge data-testid="web-fetches" variant="outline">
            {a.webFetchRequests} web fetch(es)
          </Badge>
        ) : null}
        {a.gitBranch !== undefined && a.gitBranch !== "" ? (
          <Badge variant="secondary">{a.gitBranch}</Badge>
        ) : null}
      </div>
    </section>
  );
};
