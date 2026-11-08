/** History search box: query, match counter and previous / next controls.
 *
 * Enter steps to the next hit and Shift+Enter to the previous one; focus stays
 * in the input, as in browser find. Escape clears the query. The counter is a
 * polite live region, mounted even when empty so screen readers announce the
 * first count.
 */
import { Button } from "@workspace/ui/components/button";
import { Input } from "@workspace/ui/components/input";
import { ChevronDownIcon, ChevronUpIcon, XIcon } from "lucide-react";
import type { KeyboardEvent } from "react";
import type { StepDir } from "../../lib/history-search";
import { matchLabel } from "../../lib/history-search";
import {
  CURRENT_ATTR,
  CURRENT_HIGHLIGHT,
  HIT_HIGHLIGHT,
} from "../../lib/use-search-highlight";

const HIGHLIGHT_CSS = `
::highlight(${HIT_HIGHLIGHT}) { background-color: rgb(250 204 21 / 0.35); }
::highlight(${CURRENT_HIGHLIGHT}) { background-color: rgb(249 115 22 / 0.85); color: #000; }
[${CURRENT_ATTR}] { outline: 2px solid rgb(249 115 22 / 0.8); outline-offset: 2px; border-radius: 0.375rem; }
`;

export const HistorySearch = ({
  count,
  current,
  onClear,
  onQuery,
  onStep,
  query,
}: {
  /** Number of hits for the active query. */
  readonly count: number;
  /** 0-based index of the current hit, or -1 before the first step. */
  readonly current: number;
  readonly onClear: () => void;
  readonly onQuery: (query: string) => void;
  readonly onStep: (dir: StepDir) => void;
  readonly query: string;
}) => {
  const active = query.length > 0;
  const canStep = active && count > 0;
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (canStep) {
        onStep(e.shiftKey ? -1 : 1);
      }
    } else if (e.key === "Escape" && active) {
      e.preventDefault();
      onClear();
    }
  };
  return (
    <div
      className="flex w-full items-center gap-1 sm:w-auto"
      data-testid="history-search-bar"
    >
      <style href="peektrace-history-search" precedence="default">
        {HIGHLIGHT_CSS}
      </style>
      <div className="relative min-w-0 flex-1 sm:w-64 sm:flex-none">
        <Input
          aria-label="Search history"
          className="pr-7 [&::-webkit-search-cancel-button]:hidden"
          data-testid="history-search"
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search history…"
          type="search"
          value={query}
        />
        {active ? (
          <button
            aria-label="Clear search"
            className="absolute top-1/2 right-1.5 -translate-y-1/2 rounded p-0.5 text-muted-foreground hover:text-foreground"
            data-testid="history-search-clear"
            onClick={onClear}
            type="button"
          >
            <XIcon className="size-3.5" />
          </button>
        ) : null}
      </div>
      <span
        aria-live="polite"
        className="shrink-0 whitespace-nowrap text-muted-foreground text-xs tabular-nums empty:hidden"
        data-testid="history-match-count"
        role="status"
      >
        {active ? matchLabel({ count, current }) : ""}
      </span>
      {active ? (
        <>
          <Button
            aria-label="Previous match"
            data-testid="history-search-prev"
            disabled={!canStep}
            onClick={() => onStep(-1)}
            size="icon-sm"
            variant="ghost"
          >
            <ChevronUpIcon className="size-4" />
          </Button>
          <Button
            aria-label="Next match"
            data-testid="history-search-next"
            disabled={!canStep}
            onClick={() => onStep(1)}
            size="icon-sm"
            variant="ghost"
          >
            <ChevronDownIcon className="size-4" />
          </Button>
        </>
      ) : null}
    </div>
  );
};
