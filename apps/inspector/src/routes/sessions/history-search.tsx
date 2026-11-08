/** History search box: query, match counter and previous / next controls.
 *
 * Enter steps to the next hit and Shift+Enter to the previous one; focus stays
 * in the input, as in browser find. Escape clears the query. The counter is a
 * polite live region that stays in the accessibility tree when empty (only
 * visually hidden), so screen readers announce the first count. While the
 * filter catches up with the typed query the counter stays blank, so it never
 * reads out the previous query's count.
 */
import { Button } from "@workspace/ui/components/button";
import { Input } from "@workspace/ui/components/input";
import { cn } from "@workspace/ui/lib/utils";
import { ChevronDownIcon, ChevronUpIcon, XIcon } from "lucide-react";
import { type KeyboardEvent, useRef } from "react";
import type { StepDir } from "../../lib/history-search";
import { matchLabel } from "../../lib/history-search";
import {
  CURRENT_ATTR,
  CURRENT_HIGHLIGHT,
  HIT_HIGHLIGHT,
} from "../../lib/use-search-highlight";

// Cards sit in `content-visibility: auto` wrappers, which clip paint: the
// outline is drawn inside the card so it stays visible.
const HIGHLIGHT_CSS = `
::highlight(${HIT_HIGHLIGHT}) { background-color: rgb(250 204 21 / 0.35); }
::highlight(${CURRENT_HIGHLIGHT}) { background-color: rgb(249 115 22 / 0.85); color: #000; }
[${CURRENT_ATTR}] { outline: 2px solid rgb(249 115 22 / 0.8); outline-offset: -2px; border-radius: 0.75rem; }
`;

/** The keyCode browsers report for any key pressed during IME composition. */
const IME_KEY_CODE = 229;

/** What a key press in the search box does. Enter that confirms an IME
 *  candidate (`isComposing`, or the legacy IME keyCode) is left to the IME. */
export const searchKeyAction = (e: {
  readonly isComposing: boolean;
  readonly key: string;
  readonly keyCode: number;
  readonly shiftKey: boolean;
}) => {
  if (e.isComposing || e.keyCode === IME_KEY_CODE) {
    return null;
  }
  if (e.key === "Enter") {
    return e.shiftKey ? "prev" : "next";
  }
  return e.key === "Escape" ? "clear" : null;
};

export const HistorySearch = ({
  count,
  current,
  onClear,
  onQuery,
  onStep,
  pending,
  query,
}: {
  /** Number of hits for the active query. */
  readonly count: number;
  /** 0-based index of the current hit, or -1 before the first step. */
  readonly current: number;
  readonly onClear: () => void;
  readonly onQuery: (query: string) => void;
  readonly onStep: (dir: StepDir) => void;
  /** True while `count` still belongs to an earlier query. */
  readonly pending: boolean;
  readonly query: string;
}) => {
  const inputRef = useRef<HTMLInputElement>(null);
  const active = query.length > 0;
  const canStep = active && !pending && count > 0;
  const label = active && !pending ? matchLabel({ count, current }) : "";
  const clear = () => {
    onClear();
    inputRef.current?.focus();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const action = searchKeyAction({
      isComposing: e.nativeEvent.isComposing,
      key: e.key,
      keyCode: e.keyCode,
      shiftKey: e.shiftKey,
    });
    if (action === "next" || action === "prev") {
      e.preventDefault();
      if (canStep) {
        onStep(action === "prev" ? -1 : 1);
      }
    } else if (action === "clear" && active) {
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
          className="pr-8 [&::-webkit-search-cancel-button]:hidden"
          data-testid="history-search"
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search history…"
          ref={inputRef}
          type="search"
          value={query}
        />
        {active ? (
          <button
            aria-label="Clear search"
            className="absolute top-1/2 right-0.5 -translate-y-1/2 rounded p-1 text-muted-foreground hover:text-foreground"
            data-testid="history-search-clear"
            onClick={clear}
            type="button"
          >
            <XIcon className="size-4" />
          </button>
        ) : null}
      </div>
      <span
        aria-busy={pending}
        aria-live="polite"
        className={cn(
          "shrink-0 whitespace-nowrap text-muted-foreground text-xs tabular-nums",
          label === "" && "sr-only"
        )}
        data-testid="history-match-count"
        role="status"
      >
        {label}
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
