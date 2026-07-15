/** Sessions section (Phase 8) — master-detail browser + context-debug viewer.
 *
 * A responsive two-pane layout: a compact session rail (`sessions.list`, with
 * filter/search) on the left and the full debug view (`sessions.analyze`) on the
 * right — peak gauge, budget-at-peak, growth timeline, loaded artifacts and the
 * redacted-by-default history. On `md+` both panes are visible so switching
 * sessions is a single click; below `md` the panes collapse to one column
 * (list ⇄ detail) and the detail's Back button returns to the rail. Selection is
 * local state, so the rail stays mounted and returning is instant.
 */
import { useAtomValue } from "@effect-atom/atom-react";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@workspace/ui/components/empty";
import { cn } from "@workspace/ui/lib/utils";
import { MessagesSquareIcon, MousePointerClickIcon } from "lucide-react";
import { SectionHeader } from "../components/section-header";
import { sessionsListAtom } from "../lib/atoms";
import { ResultView } from "../lib/result-view";
import { closeSession, openSession, useSelectedSessionId } from "../lib/routes";
import { SessionDetail } from "./sessions/session-detail";
import { SessionList } from "./sessions/session-list";

/** Detail-pane placeholder shown on `md+` when no session is selected. */
const NoSelection = () => (
  <Empty
    className="hidden border md:flex md:min-h-[60vh]"
    data-testid="session-empty-detail"
  >
    <EmptyHeader>
      <EmptyMedia variant="icon">
        <MousePointerClickIcon />
      </EmptyMedia>
      <EmptyTitle>No session selected</EmptyTitle>
      <EmptyDescription>
        Pick a session from the list to inspect its context-budget forensics.
      </EmptyDescription>
    </EmptyHeader>
    <EmptyContent className="text-muted-foreground">
      Peak context and health verdict · budget at peak · growth timeline ·
      loaded artifacts · redacted history.
    </EmptyContent>
  </Empty>
);

/** Sessions section route: responsive master-detail. */
export const SessionsRoute = () => {
  const result = useAtomValue(sessionsListAtom);
  const selected = useSelectedSessionId();

  return (
    <div className="flex flex-col">
      <SectionHeader
        description="Browse Claude, Codex and Pi sessions and inspect context-budget forensics."
        title="Sessions"
      />
      <ResultView result={result}>
        {(headers) =>
          headers.length === 0 ? (
            <Empty className="border" data-testid="sessions-placeholder">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <MessagesSquareIcon />
                </EmptyMedia>
                <EmptyTitle>No sessions found</EmptyTitle>
                <EmptyDescription>
                  No Claude transcripts were discovered under the projects root.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div className="flex flex-col gap-6 md:flex-row md:items-start">
              <div
                className={cn(
                  "md:scrollbar-gutter-stable md:sticky md:top-6 md:max-h-[calc(100dvh-3rem)] md:w-80 md:shrink-0 md:self-start md:overflow-y-auto md:pr-1",
                  selected ? "hidden md:block" : "block"
                )}
              >
                <SessionList
                  headers={headers}
                  onOpen={openSession}
                  selectedId={selected}
                />
              </div>
              <div
                className={cn(
                  "min-w-0 flex-1",
                  selected ? "block" : "hidden md:block"
                )}
              >
                {selected ? (
                  <SessionDetail
                    id={selected}
                    key={selected}
                    onBack={closeSession}
                  />
                ) : (
                  <NoSelection />
                )}
              </div>
            </div>
          )
        }
      </ResultView>
    </div>
  );
};
