/** Session debug view (Phase 8.2 + 8.3 orchestrator).
 *
 * Fetches `sessions.analyze` for the selected id (redaction on by default) and
 * lays out the forensic surface: verdict header + peak gauge, budget-at-peak,
 * growth timeline, loaded artifacts, then the full redacted history (sortable by
 * size) with subagent drill-down. The redaction toggle flips the atom's
 * `redact` flag, which re-fetches the analysis with raw bodies.
 */
import { useAtomValue } from "@effect-atom/atom-react";
import { Button } from "@workspace/ui/components/button";
import { BudgetBar } from "@workspace/viz/components/budget-bar";
import { GrowthTimeline } from "@workspace/viz/components/growth-timeline";
import { VerdictHeader } from "@workspace/viz/components/verdict-header";
import { ArrowLeftIcon } from "lucide-react";
import { ResultView } from "../../lib/result-view";
import { openSession } from "../../lib/routes";
import { useAnalyzedSession } from "../../lib/session-atoms";
import { useSessionView } from "../../lib/session-view";
import { LoadedArtifacts } from "./loaded-artifacts";
import { SessionHistory } from "./session-history";

/** Where a subagent transcript came from, with the way back to it. */
const ParentNote = ({ parent }: { readonly parent: string }) => (
  <div
    className="flex flex-wrap items-center gap-2 rounded-md border border-border px-3 py-2 text-sm"
    data-testid="subagent-parent"
  >
    <span className="text-muted-foreground">Subagent of</span>
    <code className="text-xs">{parent}</code>
    <Button
      className="ml-auto h-6 gap-1 px-2 text-xs"
      onClick={() => openSession(parent)}
      size="sm"
      variant="outline"
    >
      <ArrowLeftIcon className="size-3" /> Open parent session
    </Button>
  </div>
);

/** The full debug view for one session id; `onBack` returns to the list. A
 * `parent` means `id` is a subagent transcript, resolved through that session. */
export const SessionDetail = ({
  id,
  onBack,
  parent,
}: {
  readonly id: string;
  readonly onBack: () => void;
  readonly parent: string | null;
}) => {
  const view = useSessionView(id);
  const atom = useAnalyzedSession({ id, parent, redact: view.state.redacted });
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
      {parent === null ? null : <ParentNote parent={parent} />}
      <ResultView result={result}>
        {(a) => (
          <div className="flex flex-col gap-4">
            <VerdictHeader a={a} />
            <BudgetBar a={a} />
            <GrowthTimeline a={a} />
            <LoadedArtifacts a={a} />
            <SessionHistory a={a} sid={id} view={view} />
          </div>
        )}
      </ResultView>
    </div>
  );
};
