/** Session debug view.
 *
 * Fetches the session summary for the selected id (redaction on by default)
 * and lays out the forensic surface: verdict header + peak gauge,
 * budget-at-peak, growth timeline, loaded artifacts, then the full redacted
 * history (sortable by size) with subagent drill-down. The summary draws at
 * once; the growth series and the history index follow, and event pages load
 * as the history scrolls. The redaction toggle flips the `redact` flag, which
 * re-fetches every part with raw bodies.
 */
import { Result } from "@effect-atom/atom-react";
import { Button } from "@workspace/ui/components/button";
import { BudgetBar } from "@workspace/viz/components/budget-bar";
import { GrowthTimeline } from "@workspace/viz/components/growth-timeline";
import {
  analyzedTitle,
  VerdictHeader,
} from "@workspace/viz/components/verdict-header";
import type { SessionJump } from "@workspace/viz/lib/session-jump";
import { ArrowDownIcon, ArrowLeftIcon } from "lucide-react";
import { memo, type RefObject, useCallback, useMemo, useRef } from "react";
import type { SessionSummary } from "../../lib/api-types.gen";
import { useTitleOverride } from "../../lib/document-title";
import {
  type JumpLanding,
  type JumpRequest,
  useHistoryJump,
} from "../../lib/history-jump";
import { snapshotsOf } from "../../lib/history-pages";
import { ResultView } from "../../lib/result-view";
import { sessionHash, useListParams } from "../../lib/routes";
import { useScrollRoot } from "../../lib/scroll-root";
import {
  type SessionKey,
  useGrowth,
  useHistoryIndex,
  useSessionSummary,
} from "../../lib/session-data";
import { type SessionView, useSessionView } from "../../lib/session-view";
import { TranscriptSourceContext } from "../../lib/transcript-source";
import { ExportButtons } from "./export-button";
import { LoadedArtifacts } from "./loaded-artifacts";
import { SessionHistory } from "./session-history";

/** Where a subagent transcript came from, with the way back to it. */
const ParentNote = ({ parent }: { readonly parent: string }) => {
  const list = useListParams();
  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border border-border px-3 py-2 text-sm"
      data-testid="subagent-parent"
    >
      <span className="text-muted-foreground">Subagent of</span>
      <code className="text-xs">{parent}</code>
      <Button
        asChild
        className="ml-auto h-6 gap-1 px-2 text-xs"
        size="sm"
        variant="outline"
      >
        <a href={sessionHash({ id: parent, parent: null }, list)}>
          <ArrowLeftIcon className="size-3" /> Open parent session
        </a>
      </Button>
    </div>
  );
};

/** Holds the growth chart's place while its series loads. */
const GrowthPending = () => (
  <section
    aria-busy="true"
    className="flex flex-col gap-2 rounded-lg border border-border p-3 md:p-4"
    data-testid="growth-timeline-pending"
  >
    <h2 className="font-semibold text-base">Context growth timeline</h2>
    <div className="aspect-[1000/440] w-full" />
  </section>
);

/** Everything below the summary cards' first paint: the growth chart and the
 *  history, each drawn as its part arrives. */
const SessionBody = ({
  k,
  landing,
  onStale,
  onSummary,
  onTurn,
  req,
  request,
  s,
  sectionRef,
  seq,
  view,
}: {
  readonly k: SessionKey;
  readonly landing: JumpLanding;
  readonly onStale: () => void;
  readonly onSummary: () => void;
  readonly onTurn: (turn: number) => void;
  readonly req: JumpRequest | null;
  readonly request: (j: SessionJump) => void;
  readonly s: SessionSummary;
  readonly sectionRef: RefObject<HTMLElement | null>;
  readonly seq: number;
  readonly view: SessionView;
}) => {
  const growthResult = useGrowth(k, s);
  const indexResult = useHistoryIndex(k, s);
  const growth = Result.isSuccess(growthResult) ? growthResult.value : null;
  const chart = useMemo(
    () => (growth === null ? null : { ...s, snapshots: snapshotsOf(growth) }),
    [growth, s]
  );
  const source = useMemo(() => ({ id: k.id, parent: k.parent }), [k]);
  return (
    <div className="flex flex-col gap-4">
      <VerdictHeader a={s} onJump={request} />
      <ExportButtons k={k} />
      <BudgetBar a={s} />
      {chart === null ? (
        <GrowthPending />
      ) : (
        <GrowthTimeline a={chart} onTurn={onTurn} />
      )}
      <LoadedArtifacts a={s} />
      <TranscriptSourceContext.Provider value={source}>
        <ResultView result={indexResult}>
          {(index) => (
            <SessionHistory
              growth={growth}
              index={index}
              k={k}
              landing={landing}
              onStale={onStale}
              onSummary={onSummary}
              req={req}
              s={s}
              sectionRef={sectionRef}
              seq={seq}
              sid={k.id}
              view={view}
            />
          )}
        </ResultView>
      </TranscriptSourceContext.Provider>
    </div>
  );
};

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
  const redact = view.state.redacted;
  const k = useMemo(() => ({ id, parent, redact }), [id, parent, redact]);
  const { refresh: retry, result } = useSessionSummary(k);
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
  useTitleOverride(
    Result.isSuccess(result) ? analyzedTitle(result.value) : null
  );

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
      <ResultView onRetry={retry} result={result}>
        {(s) => (
          <SessionBody
            k={k}
            landing={landing}
            onStale={retry}
            onSummary={toSummary}
            onTurn={onTurn}
            req={req}
            request={request}
            s={s}
            sectionRef={history}
            seq={seq}
            view={view}
          />
        )}
      </ResultView>
    </div>
  );
};

/** Memoized: its props are stable, so a list reload leaves the transcript alone. */
export const SessionDetail = memo(SessionDetailView);
