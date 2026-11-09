/** Stats first load: a skeleton shaped like the report, an honest note on why
 *  it takes a while, and the seconds elapsed. The server sends no progress, so
 *  nothing here pretends to measure it. */
import { Skeleton } from "@workspace/ui/components/skeleton";
import { useEffect, useState } from "react";

const TICK_MS = 1000;

/** Whole seconds since `since` (epoch ms), ticking once a second. */
const useElapsedSeconds = (since: number) => {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(id);
  }, []);
  return Math.max(0, Math.floor((now - since) / TICK_MS));
};

/** False on the first render, true right after mount. A live region only
 * announces text added after it exists, so the note is filled in late. */
const useMounted = () => {
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  return mounted;
};

const TableBlock = () => (
  <div className="flex flex-col gap-3">
    <Skeleton className="h-5 w-40" />
    <Skeleton className="h-28 w-full" />
  </div>
);

/** `since` is when this scope's pass started, so a remount keeps counting. */
export const StatsPending = ({ since }: { readonly since: number }) => {
  const seconds = useElapsedSeconds(since);
  const announce = useMounted();
  return (
    <div className="flex flex-col gap-8" data-testid="stats-pending">
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Skeleton className="h-5 w-full max-w-md" />
          <Skeleton className="h-7 w-72" />
        </div>
        <div className="flex items-baseline gap-2 text-muted-foreground text-sm">
          <p role="status">
            {announce
              ? "Reading transcripts. The first pass parses every file and can take about 20 s; later passes read the cache."
              : null}
          </p>
          <span
            aria-hidden="true"
            className="shrink-0 font-mono text-xs tabular-nums"
            data-testid="stats-elapsed"
          >
            {seconds} s
          </span>
        </div>
      </div>
      <div className="flex flex-col gap-3">
        <Skeleton className="h-5 w-48" />
        <Skeleton className="h-20 w-full" />
      </div>
      <TableBlock />
      <TableBlock />
      <TableBlock />
    </div>
  );
};
