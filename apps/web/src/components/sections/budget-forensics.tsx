import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { BudgetBar } from "@workspace/viz/components/budget-bar";
import { VerdictHeader } from "@workspace/viz/components/verdict-header";
import { fmt, PERCENT } from "@workspace/viz/lib/session-format";
import { MOCK_SESSION } from "@workspace/viz/mock/session";
import { Ghost } from "lucide-react";
import { VizSurface } from "@/components/viz-surface";

const CATEGORY_COUNT = MOCK_SESSION.budget.length;
const THINKING_TOKENS =
  MOCK_SESSION.budget.find((slice) => slice.key === "thinking")?.tokens ?? 0;
const THINKING_PCT = Math.round(
  (PERCENT * THINKING_TOKENS) / MOCK_SESSION.peakContextTokens
);

/**
 * Session-forensics section: renders the inspector's own verdict header (health
 * word, peak gauge with the dumb-zone marker, session metadata) and budget bar
 * (stacked slices plus the per-category table) against the sample session, then
 * calls out the recovered thinking band. Every figure is drawn from the sample
 * data by the components themselves — nothing is restated by hand.
 */
export const BudgetForensics = () => (
  <section className="bg-muted/30 py-24" id="budget-forensics">
    <div className="mx-auto w-full max-w-6xl px-6">
      <p className="font-mono text-primary text-sm tracking-[0.2em]">
        SESSION FORENSICS · peektrace sessions analyze
      </p>
      <h2 className="mt-4 text-balance font-heading text-3xl md:text-4xl">
        Where the window actually went.
      </h2>
      <p className="mt-4 max-w-3xl text-muted-foreground">
        Peektrace finds the fullest turn — usually mid-session, not the end —
        and splits it into {CATEGORY_COUNT} categories that sum to the real
        measured size. Here: every one of {fmt(MOCK_SESSION.peakContextTokens)}{" "}
        tokens, accounted for.
      </p>

      <VizSurface
        caption="Illustrative sample — not a captured run"
        className="mt-10"
        label="peektrace inspector"
      >
        <VerdictHeader a={MOCK_SESSION} />
        <BudgetBar a={MOCK_SESSION} />
      </VizSurface>

      <Alert className="mt-6 border-primary">
        <Ghost aria-hidden="true" className="size-4" />
        <AlertTitle className="font-heading">
          The thinking tax is real.
        </AlertTitle>
        <AlertDescription>
          Claude stores retained reasoning as empty strings — invisible in the
          transcript, but it still eats the window. Peektrace reconstructs it
          from ground-truth usage. In this session it&apos;s the single largest
          slice: {THINKING_PCT}% of peak.
        </AlertDescription>
      </Alert>

      <p className="mt-6 font-mono text-muted-foreground text-xs">
        Full-fidelity attribution for Claude. Ground-truth usage for Codex
        (authoritative window) and Pi (model-inferred window).
      </p>
    </div>
  </section>
);
