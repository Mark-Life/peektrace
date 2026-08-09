/** Start-date range filter for the session rail.
 *
 * A `Calendar` in `range` mode behind a `Popover`, replacing the flat list of
 * every distinct session day. Days are compared as `YYYY-MM-DD` strings in the
 * local zone (see `toDayKey`) rather than as instants, so a session started at
 * 23:50 stays on the day the transcript recorded it. Selection is clamped to the
 * span the loaded headers actually cover.
 */
import { Button } from "@workspace/ui/components/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@workspace/ui/components/popover";
import { cn } from "@workspace/ui/lib/utils";
import { CalendarIcon, XIcon } from "lucide-react";
import { lazy, Suspense } from "react";
import type { DateRange } from "react-day-picker";

/** `react-day-picker` and its `date-fns` dependency are ~66 KB, and the sessions
 *  rail is the first thing the app paints. The popover only mounts its content
 *  when opened, so the chunk is fetched the first time someone filters by date;
 *  the type-only `DateRange` import above is erased and costs nothing. */
const Calendar = lazy(() =>
  import("@workspace/ui/components/calendar").then((m) => ({
    default: m.Calendar,
  }))
);

/** Reserves the calendar's footprint so the popover doesn't resize on load. */
const CalendarFallback = () => (
  <div aria-busy="true" className="size-64" role="status">
    <span className="sr-only">Loading calendar…</span>
  </div>
);

/** Zero-padded to two digits. */
const pad = (n: number): string => String(n).padStart(2, "0");

/** Local-calendar `YYYY-MM-DD` of a `Date` — never UTC, so no midnight drift. */
export const toDayKey = (d: Date): string =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

/** `YYYY-MM-DD` back to a local midnight `Date`, or `undefined` if unparseable. */
const fromDayKey = (key: string): Date | undefined => {
  const [y, m, d] = key.split("-").map(Number);
  return y && m && d ? new Date(y, m - 1, d) : undefined;
};

/** `Jul 9` in the user's locale. */
const fmtDay = (d: Date): string =>
  d.toLocaleDateString(undefined, { month: "short", day: "numeric" });

/** The trigger's label: the picked span, one day, or the unconstrained default. */
const rangeLabel = (range: DateRange | undefined): string => {
  if (!range?.from) {
    return "Date: all";
  }
  if (!range.to || toDayKey(range.to) === toDayKey(range.from)) {
    return fmtDay(range.from);
  }
  return `${fmtDay(range.from)} – ${fmtDay(range.to)}`;
};

/** Popover calendar constrained to `days`; `onChange(undefined)` clears the facet. */
export const DateRangeFilter = ({
  days,
  range,
  onChange,
}: {
  /** Every `YYYY-MM-DD` a loaded session started on, ascending. */
  readonly days: readonly string[];
  readonly range: DateRange | undefined;
  readonly onChange: (range: DateRange | undefined) => void;
}) => {
  const first = days[0] ? fromDayKey(days[0]) : undefined;
  const last = days.at(-1) ? fromDayKey(days.at(-1) as string) : undefined;
  const selected = Boolean(range?.from);

  return (
    <Popover>
      <div className="relative col-span-2">
        <PopoverTrigger asChild>
          <Button
            className={cn(
              "w-full justify-start font-normal",
              selected ? "pr-8" : undefined,
              selected || "text-muted-foreground"
            )}
            data-testid="session-filter-date"
            variant="outline"
          >
            <CalendarIcon />
            {rangeLabel(range)}
          </Button>
        </PopoverTrigger>
        {selected ? (
          <Button
            aria-label="Clear date filter"
            className="absolute top-1/2 right-1 size-6 -translate-y-1/2 text-muted-foreground"
            data-testid="session-filter-date-clear"
            onClick={() => onChange(undefined)}
            size="icon"
            variant="ghost"
          >
            <XIcon />
          </Button>
        ) : null}
      </div>
      <PopoverContent align="start" className="w-auto p-0">
        <Suspense fallback={<CalendarFallback />}>
          <Calendar
            autoFocus
            defaultMonth={range?.from ?? last}
            disabled={
              first && last
                ? { before: first, after: last }
                : { before: new Date(0) }
            }
            endMonth={last}
            mode="range"
            onSelect={onChange}
            selected={range}
            startMonth={first}
          />
        </Suspense>
      </PopoverContent>
    </Popover>
  );
};
