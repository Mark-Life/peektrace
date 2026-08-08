/** The quiet freshness indicator, and the home of the watch poll.
 *
 * Polling and the refresh fan-out live here rather than in `App` so a disk change
 * re-renders one dot instead of the whole tree (a 260-row transcript re-rendering
 * twice a second is what made the live view unreadable). The signal itself sits
 * in the shell header — outside every data surface — so it can never wrap, dim,
 * or unmount the content it is reporting on.
 *
 * Layout is fixed: the label is always present and only its opacity changes, so
 * a sync never shifts the header.
 */
import { cn } from "@workspace/ui/lib/utils";
import { useEffect, useState } from "react";
import { useWatchRefresh } from "../lib/watch-atoms";

/** How long the dot stays lit after a bump (ms) — long enough to notice. */
const FLASH_MS = 1200;

/** Small "syncing" dot, lit briefly whenever a watched scope changes on disk. */
export const WatchStatus = () => {
  const bumps = useWatchRefresh();
  const [syncing, setSyncing] = useState(false);

  useEffect(() => {
    if (bumps === 0) {
      return;
    }
    setSyncing(true);
    const id = setTimeout(() => setSyncing(false), FLASH_MS);
    return () => clearTimeout(id);
  }, [bumps]);

  return (
    <span
      aria-hidden="true"
      className="ml-auto flex shrink-0 items-center gap-1.5 text-muted-foreground text-xs"
      data-syncing={syncing ? "true" : "false"}
      data-testid="watch-status"
      title="Peektrace re-reads a file as soon as an agent writes it"
    >
      <span
        className={cn(
          "size-1.5 rounded-full transition-colors duration-500",
          syncing ? "bg-primary" : "bg-transparent"
        )}
      />
      <span
        className={cn(
          "transition-opacity duration-500",
          syncing ? "opacity-100" : "opacity-0"
        )}
      >
        syncing
      </span>
    </span>
  );
};
