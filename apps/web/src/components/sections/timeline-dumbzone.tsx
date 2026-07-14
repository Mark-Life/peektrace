import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@workspace/ui/components/card";
import {
  GROWTH_TIMELINE_MIN_WIDTH,
  GROWTH_TIMELINE_WIDTH,
  GrowthTimeline,
} from "@workspace/viz/components/growth-timeline";
import { fmt, PERCENT } from "@workspace/viz/lib/session-format";
import { MOCK_SESSION } from "@workspace/viz/mock/session";
import { AlertTriangle, Diamond, Scissors } from "lucide-react";
import { VizSurface } from "@/components/viz-surface";

const DUMB_ZONE_PCT = Math.round(MOCK_SESSION.dumbZoneFraction * PERCENT);
/** Turn numbers are 1-based in the UI; the analyzer stores 0-based indexes. */
const CROSS_TURN = MOCK_SESSION.dumbZoneCrossTurn + 1;
const PEAK_TURN = MOCK_SESSION.peakTurnIndex + 1;
const COMPACTION_COUNT = MOCK_SESSION.compactionTurns.length;

/**
 * Explanatory chips beneath the timeline. Titles pair with a lucide glyph; the
 * dumb-zone cutoff, crossing turn, and peak are read from the sample session, so
 * the prose can never drift from what the chart draws.
 */
const CHIPS = [
  {
    icon: AlertTriangle,
    title: "Dumb zone",
    body: `Usage at or above ${DUMB_ZONE_PCT}% of the window. Context-rot territory. In this sample you cross it at turn ${CROSS_TURN} and spend ${MOCK_SESSION.dumbZoneTurns} of ${MOCK_SESSION.turnCount} turns there.`,
  },
  {
    icon: Scissors,
    title: "Compaction cliff",
    body: `History summarized away. Detail discussed before the cliff may be gone from context — this session takes ${COMPACTION_COUNT}.`,
  },
  {
    icon: Diamond,
    title: "Peak",
    body: `The most-loaded turn: ${fmt(MOCK_SESSION.peakContextTokens)} tokens at turn ${PEAK_TURN}. The last turn can look small after a compaction and hide how close you ran to the wall.`,
  },
] as const;

/**
 * Context-growth timeline section: the inspector's stacked-area chart of real
 * per-turn context, held between its native geometry width and a legibility
 * floor so the axis labels and hairline gridlines render at the size they were
 * drawn for, and scroll sideways rather than shrink on narrow viewports.
 */
export const TimelineDumbzone = () => (
  <section className="py-20" id="timeline-dumbzone">
    <div className="mx-auto max-w-6xl px-4 sm:px-6">
      <p className="font-mono text-muted-foreground text-xs uppercase tracking-[0.2em]">
        CONTEXT GROWTH TIMELINE
      </p>
      <h2 className="mt-4 text-balance font-heading font-semibold text-3xl tracking-tight sm:text-4xl">
        Watch context climb into the dumb zone — and see every cliff where
        history got dropped.
      </h2>
      <p className="mt-4 max-w-2xl text-pretty text-muted-foreground">
        A per-turn stacked-area chart of real context, turn 1 to the ceiling. A
        red danger band marks the dumb zone (context at or above {DUMB_ZONE_PCT}
        % of the window, where attention and quality quietly degrade). Sharp
        downward cliffs mark compactions — the moments the agent summarized and
        evicted growable history. A diamond marks peak; a marker flags the exact
        turn context first crossed into the danger band.
      </p>

      <VizSurface
        caption="Illustrative sample — not a captured run"
        className="mt-8"
        label="peektrace inspector"
      >
        <GrowthTimeline
          a={MOCK_SESSION}
          maxWidth={GROWTH_TIMELINE_WIDTH}
          minWidth={GROWTH_TIMELINE_MIN_WIDTH}
        />
      </VizSurface>

      <ul className="mt-8 grid gap-4 md:grid-cols-3">
        {CHIPS.map(({ icon: Icon, title, body }) => (
          <li key={title}>
            <Card className="h-full">
              <CardHeader>
                <CardTitle className="flex items-center gap-2 font-heading">
                  <Icon aria-hidden="true" className="size-4 text-primary" />
                  {title}
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-pretty text-muted-foreground text-sm">
                  {body}
                </p>
              </CardContent>
            </Card>
          </li>
        ))}
      </ul>
    </div>
  </section>
);
