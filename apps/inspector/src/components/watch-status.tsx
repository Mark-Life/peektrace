/** The quiet freshness indicator, the "Pause live" toggle, and the home of the
 * watch poll.
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
import { useAtom, useAtomValue } from "@effect-atom/atom-react";
import { Button } from "@workspace/ui/components/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";
import { cn } from "@workspace/ui/lib/utils";
import { PauseIcon, PlayIcon } from "lucide-react";
import { useEffect, useId, useState } from "react";
import {
  livePausedAtom,
  liveWaitingAtom,
  useWatchRefresh,
} from "../lib/watch-atoms";

/** How long the dot stays lit after a bump (ms) — long enough to notice. */
const FLASH_MS = 1200;

/** Live updates flowing, paused with nothing new, or paused with changes held. */
export type LiveMode = "live" | "paused" | "waiting";

const LABEL: Record<LiveMode, string> = {
  live: "syncing",
  paused: "paused",
  waiting: "paused · updates waiting",
};

const liveMode = (paused: boolean, waiting: boolean) => {
  if (!paused) {
    return "live";
  }
  return waiting ? "waiting" : "paused";
};

/** The dot, label and toggle, with no data wiring. The toggle keeps one fixed
 *  name and reports its state through `aria-pressed`; while paused, the label
 *  is its description, so "updates waiting" reaches screen readers too. */
export const WatchStatusView = ({
  mode,
  syncing,
  onToggle,
}: {
  readonly mode: LiveMode;
  readonly syncing: boolean;
  readonly onToggle: () => void;
}) => {
  const paused = mode !== "live";
  const labelId = useId();
  const hint = paused ? "Resume live updates" : "Pause live updates";
  const Icon = paused ? PlayIcon : PauseIcon;
  return (
    <div
      className="ml-auto flex shrink-0 items-center gap-1.5 text-muted-foreground text-xs"
      data-live={mode}
      data-syncing={syncing ? "true" : "false"}
      data-testid="watch-status"
    >
      <span
        className="flex items-center gap-1.5"
        title="Peektrace re-reads a file as soon as an agent writes it"
      >
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 rounded-full transition-colors duration-500",
            paused && "border border-muted-foreground",
            !paused && syncing ? "bg-primary" : "bg-transparent"
          )}
        />
        <span
          aria-hidden={paused ? undefined : "true"}
          className={cn(
            "whitespace-nowrap transition-opacity duration-500",
            paused || syncing ? "opacity-100" : "opacity-0"
          )}
        >
          <span id={labelId}>{LABEL[mode]}</span>
        </span>
      </span>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            aria-describedby={paused ? labelId : undefined}
            aria-label="Pause live updates"
            aria-pressed={paused}
            data-testid="watch-pause"
            onClick={onToggle}
            size="icon-sm"
            variant="ghost"
          >
            <Icon />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="bottom">{hint}</TooltipContent>
      </Tooltip>
    </div>
  );
};

/** Small "syncing" dot, lit briefly whenever a watched scope changes on disk,
 *  next to a toggle that pauses every live refresh. */
export const WatchStatus = () => {
  const bumps = useWatchRefresh();
  const [paused, setPaused] = useAtom(livePausedAtom);
  const waiting = useAtomValue(liveWaitingAtom);
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
    <WatchStatusView
      mode={liveMode(paused, waiting)}
      onToggle={() => setPaused((p) => !p)}
      syncing={syncing}
    />
  );
};
