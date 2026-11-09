/** Session debug view (Phase 8.2 + 8.3 orchestrator).
 *
 * Fetches `sessions.analyze` for the selected id (redaction on by default) and
 * lays out the forensic surface: verdict header + peak gauge, budget-at-peak,
 * growth timeline, loaded artifacts, then the full redacted history (sortable by
 * size) with subagent drill-down. The redaction toggle flips the atom's
 * `redact` flag, which re-fetches the analysis with raw bodies.
 */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import { Button } from "@workspace/ui/components/button";
import { BudgetBar } from "@workspace/viz/components/budget-bar";
import { GrowthTimeline } from "@workspace/viz/components/growth-timeline";
import { VerdictHeader } from "@workspace/viz/components/verdict-header";
import { ArrowDownIcon, ArrowLeftIcon } from "lucide-react";
import { memo, useCallback, useMemo, useRef } from "react";
import { useHistoryJump } from "../../lib/history-jump";
import { ResultView } from "../../lib/result-view";
import { openSession } from "../../lib/routes";
import { useScrollRoot } from "../../lib/scroll-root";
import { useAnalyzedSession } from "../../lib/session-atoms";
import { useSessionView } from "../../lib/session-view";
import { TranscriptSourceContext } from "../../lib/transcript-source";
import { ExportButtons } from "./export-button";
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
const SessionDetailView = ({
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
  const source = useMemo(() => ({ id, parent }), [id, parent]);
  const root = useScrollRoot();
  const history = useRef<HTMLElement>(null);
  const jumpButton = useRef<HTMLButtonElement>(null);
  const { landing, req, seq, request } = useHistoryJump();
  const onTurn = useCallback(
    (turn: number) => request({ kind: "turn", turn }),
    [request]
  );

  // Scrolled by offset, not `scrollIntoView`: that would honour the pane's
  // `scroll-padding-top`, kept for the toolbar, which is not stuck up here.
  const toHistory = () => {
    const section = history.current;
    if (!section) {
      return;
    }
    if (root) {
      const top =
        section.getBoundingClientRect().top -
        root.getBoundingClientRect().top +
        root.scrollTop;
      root.scrollTo({ top });
    } else {
      section.scrollIntoView({ block: "start" });
    }
    section.querySelector("h2")?.focus({ preventScroll: true });
  };
  const toSummary = useCallback(() => {
    root?.scrollTo({ top: 0 });
    jumpButton.current?.focus({ preventScroll: true });
  }, [root]);

  return (
    <div className="flex flex-col gap-4" data-testid="session-detail">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          className="w-fit md:hidden"
          data-testid="session-back"
          onClick={onBack}
          size="sm"
          variant="ghost"
        >
          <ArrowLeftIcon className="size-4" /> Back to sessions
        </Button>
        {Result.isSuccess(result) ? (
          <Button
            className="w-fit"
            data-testid="jump-to-history"
            onClick={toHistory}
            ref={jumpButton}
            size="sm"
            variant="ghost"
          >
            Jump to history <ArrowDownIcon className="size-4" />
          </Button>
        ) : null}
      </div>
      {parent === null ? null : <ParentNote parent={parent} />}
      <ResultView result={result}>
        {(a) => (
          <div className="flex flex-col gap-4">
            <VerdictHeader a={a} onJump={request} />
            <ExportButtons a={a} redacted={view.state.redacted} />
            <BudgetBar a={a} />
            <GrowthTimeline a={a} onTurn={onTurn} />
            <LoadedArtifacts a={a} />
            <TranscriptSourceContext.Provider value={source}>
              <SessionHistory
                a={a}
                landing={landing}
                onSummary={toSummary}
                req={req}
                sectionRef={history}
                seq={seq}
                sid={id}
                view={view}
              />
            </TranscriptSourceContext.Provider>
          </div>
        )}
      </ResultView>
    </div>
  );
};

/** Memoized: its props are stable, so a list reload leaves the transcript alone. */
export const SessionDetail = memo(SessionDetailView);
