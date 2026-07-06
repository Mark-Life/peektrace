/** Session debug view (Phase 8.2 + 8.3 orchestrator).
 *
 * Fetches `sessions.analyze` for the selected id (redaction on by default) and
 * lays out the forensic surface: verdict header + peak gauge, budget-at-peak,
 * growth timeline, loaded artifacts + biggest items, then the full redacted
 * history with subagent drill-down. The redaction toggle flips the atom's
 * `redact` flag, which re-fetches the analysis with raw bodies.
 */
import { useAtomValue } from "@effect-atom/atom-react";
import { Button } from "@workspace/ui/components/button";
import { ArrowLeftIcon } from "lucide-react";
import { useState } from "react";
import { ResultView } from "../../lib/result-view";
import { useAnalyzedSession } from "../../lib/session-atoms";
import { BudgetBar } from "./budget-bar";
import { GrowthTimeline } from "./growth-timeline";
import { LoadedArtifacts } from "./loaded-artifacts";
import { SessionHistory } from "./session-history";
import { VerdictHeader } from "./verdict-header";

/** The full debug view for one session id; `onBack` returns to the list. */
export const SessionDetail = ({
  id,
  onBack,
}: {
  readonly id: string;
  readonly onBack: () => void;
}) => {
  const [redacted, setRedacted] = useState(true);
  const atom = useAnalyzedSession({ id, redact: redacted });
  const result = useAtomValue(atom);

  return (
    <div className="flex flex-col gap-4" data-testid="session-detail">
      <Button
        className="w-fit md:hidden"
        data-testid="session-back"
        onClick={onBack}
        size="sm"
        variant="ghost"
      >
        <ArrowLeftIcon className="size-4" /> Back to sessions
      </Button>
      <ResultView result={result}>
        {(a) => (
          <div className="flex flex-col gap-4">
            <VerdictHeader a={a} />
            <BudgetBar a={a} />
            <GrowthTimeline a={a} />
            <LoadedArtifacts a={a} />
            <SessionHistory
              a={a}
              onToggleRedact={(next) => setRedacted(next)}
              redacted={redacted}
            />
          </div>
        )}
      </ResultView>
    </div>
  );
};
