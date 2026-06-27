/** Sessions section (Phase 8) — browser + context-debug viewer.
 *
 * Lists Claude sessions from `sessions.list` (filter/search, lazy headers);
 * selecting a row opens the full debug view (`sessions.analyze`) with the peak
 * gauge, budget-at-peak, growth timeline, loaded artifacts and the redacted-by-
 * default history. Selection is local component state — the list stays mounted
 * so returning is instant.
 */
import { useAtomValue } from "@effect-atom/atom-react";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@workspace/ui/components/empty";
import { MessagesSquareIcon } from "lucide-react";
import { useState } from "react";
import { SectionHeader } from "../components/section-header";
import { sessionsListAtom } from "../lib/atoms";
import { ResultView } from "../lib/result-view";
import { SessionDetail } from "./sessions/session-detail";
import { SessionList } from "./sessions/session-list";

/** Sessions section route: list ⇄ debug-view switch. */
export const SessionsRoute = () => {
  const result = useAtomValue(sessionsListAtom);
  const [selected, setSelected] = useState<string | null>(null);

  if (selected) {
    return (
      <div>
        <SectionHeader
          description="Context-budget forensics: peak gauge, budget-at-peak, growth timeline, full history."
          title="Session debug"
        />
        <SessionDetail id={selected} onBack={() => setSelected(null)} />
      </div>
    );
  }

  return (
    <div>
      <SectionHeader
        description="Browse Claude sessions and inspect context-budget forensics."
        title="Sessions"
      />
      <ResultView result={result}>
        {(headers) =>
          headers.length === 0 ? (
            <Empty data-testid="sessions-placeholder">
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
            <SessionList headers={headers} onOpen={setSelected} />
          )
        }
      </ResultView>
    </div>
  );
};
