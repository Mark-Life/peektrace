/** The calls behind one aggregate row, in a side sheet.
 *
 * The row is the claim; this is the evidence. Every hit is one tool call with
 * its session, its time and the line it ran, and clicking one opens that
 * transcript at the detector that sent you there. The open key lives in the
 * hash, so a drill is a shareable link.
 *
 * A row can hold thousands of calls, so the sheet grows as it is scrolled
 * instead of mounting the page at once. Reaching past the fetched page is the
 * one action that costs another corpus pass, so it is a button, never a scroll.
 */
import type { Atom, Result } from "@effect-atom/atom-react";
import { useAtomValue } from "@effect-atom/atom-react";
import type {
  DrillHit,
  DrillResult,
} from "@workspace/core/services/stats/schema";
import type { WireError } from "@workspace/rpc/contract";
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@workspace/ui/components/sheet";
import { type UIEvent, useState } from "react";
import { ResultView } from "../../lib/result-view";
import { openSessionMarked } from "../../lib/routes";
import { DRILL_PAGE } from "../../lib/stats-atoms";
import { hms, n } from "../../lib/stats-format";

/** One open drill, as the sheet reads it. */
type DrillAtom = Atom.Atom<Result.Result<DrillResult, WireError>>;

/** Chars of a session id a hit shows. */
const SID_HEAD = 8;

/** Hits mounted at once, and added each time the list is scrolled to its end. */
const STEP = 30;

/** Distance from the bottom, in px, that counts as "scrolled to the end". */
const NEAR_END = 240;

/** One call: when, where, how long, and whether it failed. */
const Hit = ({
  hit,
  detector,
}: {
  readonly detector: string | null;
  readonly hit: DrillHit;
}) => (
  <button
    className="flex w-full flex-col gap-1 rounded-md border border-border px-2 py-1.5 text-left hover:bg-muted/50"
    data-testid="drill-hit"
    onClick={() => openSessionMarked(hit.sid, detector ?? "")}
    type="button"
  >
    <div className="flex items-baseline gap-2 text-xs">
      <span className="font-medium">{hit.tool}</span>
      {hit.failed ? (
        <Badge className="shrink-0" variant="destructive">
          failed
        </Badge>
      ) : null}
      <span className="ml-auto shrink-0 font-mono text-muted-foreground">
        {hit.durSec === null ? "—" : hms(hit.durSec)}
      </span>
    </div>
    <code className="wrap-break-word whitespace-pre-wrap text-muted-foreground text-xs">
      {hit.line}
    </code>
    <span className="font-mono text-muted-foreground/70 text-xs">
      {hit.project}/{hit.sid.slice(0, SID_HEAD)} · {hit.kind} ·{" "}
      {hit.ts ?? "no timestamp"}
    </span>
  </button>
);

/** What the reader is looking at, and what is left. */
const PageNote = ({
  onPage,
  result,
}: {
  readonly onPage: (offset: number) => void;
  readonly result: DrillResult;
}) => {
  const next = result.offset + result.hits.length;
  const left = result.total - next;
  return (
    <div className="flex flex-wrap items-center gap-2 pt-1">
      <p className="text-muted-foreground text-xs">
        {n(result.offset + 1)}–{n(next)} of {n(result.total)} calls
      </p>
      {left > 0 ? (
        <Button
          className="h-6 px-2 text-xs"
          onClick={() => onPage(next)}
          size="sm"
          variant="outline"
        >
          Load {n(Math.min(left, DRILL_PAGE))} more
        </Button>
      ) : null}
      {result.offset > 0 ? (
        <Button
          className="h-6 px-2 text-xs"
          onClick={() => onPage(Math.max(0, result.offset - DRILL_PAGE))}
          size="sm"
          variant="ghost"
        >
          Back
        </Button>
      ) : null}
    </div>
  );
};

/** The hits of one drill page, mounted a screenful at a time. */
const Hits = ({
  detector,
  onPage,
  result,
}: {
  readonly detector: string | null;
  readonly onPage: (offset: number) => void;
  readonly result: DrillResult;
}) => {
  const [shown, setShown] = useState(STEP);
  const visible = result.hits.slice(0, shown);
  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_END;
    if (atEnd && shown < result.hits.length) {
      setShown(shown + STEP);
    }
  };
  return (
    <div
      className="min-h-0 flex-1 overflow-y-auto"
      data-testid="drill-scroll"
      onScroll={onScroll}
    >
      <div className="flex flex-col gap-2 px-4 pb-6">
        {visible.map((hit) => (
          <Hit detector={detector} hit={hit} key={`${hit.sid}:${hit.seq}`} />
        ))}
        <PageNote onPage={onPage} result={result} />
      </div>
    </div>
  );
};

/** Reads the atom in its own component so the sheet never subscribes to null. */
const DrillBody = ({
  atom,
  detector,
  onPage,
}: {
  readonly atom: DrillAtom;
  readonly detector: string | null;
  readonly onPage: (offset: number) => void;
}) => {
  const result = useAtomValue(atom);
  return (
    <ResultView result={result}>
      {(value) => (
        <Hits
          detector={detector}
          key={value.offset}
          onPage={onPage}
          result={value}
        />
      )}
    </ResultView>
  );
};

/** The drill sheet. `atom` is null when nothing is open. */
export const DrillPanel = ({
  atom,
  detector,
  onClose,
  onPage,
  title,
}: {
  readonly atom: DrillAtom | null;
  readonly detector: string | null;
  readonly onClose: () => void;
  readonly onPage: (offset: number) => void;
  readonly title: string;
}) => (
  <Sheet
    onOpenChange={(open) => (open ? null : onClose())}
    open={atom !== null}
  >
    <SheetContent className="w-full sm:max-w-xl" data-testid="stats-drill">
      <SheetHeader>
        <SheetTitle className="truncate">{title}</SheetTitle>
        <SheetDescription>
          Every call behind this row, in the order it ran. Click one to open its
          session.
        </SheetDescription>
      </SheetHeader>
      {atom === null ? null : (
        <DrillBody atom={atom} detector={detector} onPage={onPage} />
      )}
    </SheetContent>
  </Sheet>
);
