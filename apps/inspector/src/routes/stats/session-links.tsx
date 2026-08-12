/** The route back: from an aggregate row to the sessions it came from.
 *
 * One click opens the transcript with the row's `detector` id in the hash, so
 * the session reader marks the same calls the number counted. A row with no
 * named detector still links — the transcript simply draws its own marks.
 */
import type { SampleRef } from "@workspace/core/services/stats/schema";
import { Button } from "@workspace/ui/components/button";
import { ArrowUpRightIcon } from "lucide-react";
import { openSessionMarked } from "../../lib/routes";

/** Chars of a session id a link shows; the full id is in the title. */
const SID_HEAD = 8;

/** One session button, labelled by project so two links are told apart. */
const SessionLink = ({
  detector,
  sample,
}: {
  readonly detector: string | null;
  readonly sample: SampleRef;
}) => (
  <Button
    className="h-6 gap-1 px-2 font-mono text-xs"
    data-testid="stats-session-link"
    onClick={() => openSessionMarked(sample.sid, detector ?? "")}
    size="sm"
    title={`${sample.project} · ${sample.sid} · ${sample.line}`}
    variant="outline"
  >
    <ArrowUpRightIcon className="size-3" />
    {sample.project}/{sample.sid.slice(0, SID_HEAD)}
  </Button>
);

/** Sample session links for one row, with the evidence line under them. */
export const SessionLinks = ({
  detector,
  samples,
  showLine = true,
}: {
  readonly detector: string | null;
  readonly samples: readonly SampleRef[];
  readonly showLine?: boolean;
}) => {
  if (samples.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-1" data-testid="stats-session-links">
      <div className="flex flex-wrap items-center gap-1.5">
        {samples.map((sample) => (
          <SessionLink
            detector={detector}
            key={`${sample.sid}:${sample.seq}`}
            sample={sample}
          />
        ))}
      </div>
      {showLine && samples[0] ? (
        <code className="truncate text-muted-foreground text-xs">
          {samples[0].line}
        </code>
      ) : null}
    </div>
  );
};
