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
import { sessionChip } from "../../lib/stats-format";

/** The hover text: every id behind the chip, then the evidence line. */
const titleOf = (sample: SampleRef) =>
  sample.parent === null
    ? `${sample.project} · ${sample.sid} · ${sample.line}`
    : `${sample.project} · ${sample.parent} · subagent ${sample.sid} · ${sample.line}`;

/** One session button, labelled by project so two links are told apart. A
 * subagent call names its parent session and opens the subagent through it. */
const SessionLink = ({
  detector,
  sample,
}: {
  readonly detector: string | null;
  readonly sample: SampleRef;
}) => {
  const chip = sessionChip(sample);
  return (
    <Button
      className="h-6 min-w-0 max-w-full gap-1 px-2 font-mono text-xs"
      data-testid="stats-session-link"
      onClick={() =>
        openSessionMarked({
          id: sample.sid,
          parent: sample.parent,
          mark: detector ?? "",
          call: sample.toolUseId,
        })
      }
      size="sm"
      title={titleOf(sample)}
      variant="outline"
    >
      <ArrowUpRightIcon className="size-3 shrink-0" />
      <span className="flex min-w-0">
        <span className="truncate">{chip.project}</span>
        <span className="shrink-0">/{chip.session}</span>
        {chip.subagent ? (
          <span className="shrink-0 text-muted-foreground"> · subagent</span>
        ) : null}
      </span>
    </Button>
  );
};

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
    <div
      className="flex min-w-0 max-w-full flex-col gap-1"
      data-testid="stats-session-links"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
        {samples.map((sample) => (
          <SessionLink
            detector={detector}
            key={`${sample.sid}:${sample.seq}`}
            sample={sample}
          />
        ))}
      </div>
      {showLine && samples[0] ? (
        <code
          className="block min-w-0 truncate text-muted-foreground text-xs"
          title={samples[0].line}
        >
          {samples[0].line}
        </code>
      ) : null}
    </div>
  );
};
