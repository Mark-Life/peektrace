/** Verdict header + peak gauge.
 *
 * A health word (Healthy / Degrading / Rotting) derived from peak context vs
 * window, the peak gauge with its dumb-zone marker, and the headline metadata
 * grid (peak, turns, system tax, dumb-zone dwell).
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
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

/** Short id prefix shown in the metadata line. */
const ID_PREFIX = 8;

/** Display label per transcript provider. */
const PROVIDER_LABEL: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  pi: "Pi",
};

/** A single metadata cell. */
const Meta = ({
  k,
  children,
}: {
  readonly k: string;
  readonly children: ReactNode;
}) => (
  <div className="min-w-0 rounded-md border border-border bg-card px-3 py-2">
    <div className="truncate text-muted-foreground text-xs uppercase tracking-wide">
      {k}
    </div>
    <div className="wrap-anywhere mt-0.5 text-sm">{children}</div>
  </div>
);

/** The peak gauge track: fill width = peak fraction, dumb-zone tick overlaid. */
const PeakGauge = ({ a }: { readonly a: AnalyzedSession }) => {
  const frac = a.contextWindow ? a.peakContextTokens / a.contextWindow : 0;
  const zone = zoneOf({ frac, dumbZone: a.dumbZoneFraction });
  return (
    <div className="flex flex-col gap-1" data-testid="peak-gauge">
      <div className="relative h-4 overflow-hidden rounded-full bg-muted">
        <div
          className={cn("h-full", ZONE_CLASSES[zone].fill)}
          style={{ width: `${Math.min(PERCENT, frac * PERCENT).toFixed(1)}%` }}
        />
        <div
          className="absolute top-0 h-full w-0.5 bg-foreground/70"
          style={{ left: `${(a.dumbZoneFraction * PERCENT).toFixed(0)}%` }}
          title={`dumb zone ${Math.round(a.dumbZoneFraction * PERCENT)}%`}
        />
      </div>
      <div className="flex justify-between text-muted-foreground text-xs">
        <span data-testid="peak-tokens">peak {fmt(a.peakContextTokens)}</span>
        <span>
          {fmtPct(frac)} of {fmt(a.contextWindow)}
        </span>
      </div>
    </div>
  );
};

/** Verdict header card: title, health badge, gauge, and metadata grid. */
export const VerdictHeader = ({ a }: { readonly a: AnalyzedSession }) => {
  const frac = a.contextWindow ? a.peakContextTokens / a.contextWindow : 0;
  const zone = zoneOf({ frac, dumbZone: a.dumbZoneFraction });
  const cacheFrac = a.peakContextTokens
    ? a.peakCacheReadTokens / a.peakContextTokens
    : 0;
  return (
    <section
      className="flex flex-col gap-4 rounded-lg border border-border p-4"
      data-testid="verdict-header"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1 basis-64">
          <div className="font-semibold text-lg">
            {a.title || `${PROVIDER_LABEL[a.provider] ?? a.provider} session`}
          </div>
          <div
            className="truncate font-mono text-muted-foreground text-xs"
            title={a.cwd ?? undefined}
          >
            {a.sessionId.slice(0, ID_PREFIX)} · {a.models.join(", ")} ·{" "}
            {a.cwd ?? ""}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge data-testid="session-provider" variant="outline">
            {PROVIDER_LABEL[a.provider] ?? a.provider}
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

      <div className="grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))] gap-2">
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
          <Badge className={ZONE_CLASSES.bad.text} variant="outline">
            entered dumb zone @ turn {a.dumbZoneCrossTurn + 1}
          </Badge>
        ) : (
          <Badge className={ZONE_CLASSES.ok.text} variant="outline">
            stayed in smart zone
          </Badge>
        )}
        {a.compactionTurns.length > 0 ? (
          <Badge variant="outline">
            {a.compactionTurns.length} compaction(s)
          </Badge>
        ) : null}
        {a.subagents.length > 0 ? (
          <Badge variant="outline">{a.subagents.length} subagent(s)</Badge>
        ) : null}
        {a.gitBranch ? <Badge variant="secondary">{a.gitBranch}</Badge> : null}
      </div>
    </section>
  );
};
