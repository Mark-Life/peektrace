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
import { BudgetBar } from "@workspace/viz/components/budget-bar";
import { GrowthTimeline } from "@workspace/viz/components/growth-timeline";
import { VerdictHeader } from "@workspace/viz/components/verdict-header";
import { ArrowLeftIcon } from "lucide-react";
import { ResultView } from "../../lib/result-view";
import { useAnalyzedSession } from "../../lib/session-atoms";
import { useSessionView } from "../../lib/session-view";
import { LoadedArtifacts } from "./loaded-artifacts";
import { SessionHistory } from "./session-history";

/** The full debug view for one session id; `onBack` returns to the list. */
export const SessionDetail = ({
  id,
  onBack,
}: {
  readonly id: string;
  readonly onBack: () => void;
}) => {
  const view = useSessionView(id);
  const atom = useAnalyzedSession({ id, redact: view.state.redacted });
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
            <SessionHistory a={a} view={view} />
          </div>
        )}
      </ResultView>
    </div>
  );
};
