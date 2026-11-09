/** The route back: from an aggregate row to the sessions it came from.
 *
 * One click opens the transcript with the row's `detector` id in the hash, so
 * the session reader marks the same calls the number counted. A row with no
 * named detector still links — the transcript simply draws its own marks.
 */

import { useAtomValue } from "@effect-atom/atom-react";
import type { SampleRef } from "@workspace/core/services/stats/schema";
import { Button } from "@workspace/ui/components/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@workspace/ui/components/popover";
import { ArrowUpRightIcon } from "lucide-react";
import { markedSessionHash } from "../../lib/routes";
import { sessionTitlesAtom } from "../../lib/session-titles";
import {
  chipLabelOf,
  type SessionChipGroup,
  type SessionTitles,
  sessionChips,
} from "../../lib/stats-format";

/** The hover text: every sample behind the chip, then its project. */
const titleOf = (samples: readonly SampleRef[]) =>
  samples
    .map((s) =>
      s.parent === null
        ? `${s.project} · ${s.sid} · ${s.line}`
        : `${s.project} · ${s.parent} · subagent ${s.sid} · ${s.line}`
    )
    .join("\n");

/** The link that opens one sample in the transcript, marked like the row's
 *  number. */
const sampleHash = (detector: string | null, sample: SampleRef) =>
  markedSessionHash({
    id: sample.sid,
    parent: sample.parent,
    mark: detector ?? "",
    call: sample.toolUseId,
  });

/** One link per sample of a folded chip, so each transcript stays reachable. */
export const SampleTargets = ({
  detector,
  samples,
}: {
  readonly detector: string | null;
  readonly samples: readonly SampleRef[];
}) => (
  <ul className="flex flex-col gap-1" data-testid="stats-session-targets">
    {samples.map((sample) => (
      <li key={`${sample.sid}:${sample.seq}`}>
        <Button
          asChild
          className="h-6 w-full min-w-0 justify-start gap-1 px-2 font-mono text-xs"
          size="sm"
          variant="ghost"
        >
          <a
            data-testid="stats-session-target"
            href={sampleHash(detector, sample)}
            title={sample.line}
          >
            <ArrowUpRightIcon className="size-3 shrink-0" />
            <span className="truncate">
              {sample.parent === null ? "main" : `subagent ${sample.sid}`}
              {" · "}
              {sample.line}
            </span>
          </a>
        </Button>
      </li>
    ))}
  </ul>
);

/** One session link. Samples of one session fold into it; a title names the
 * session, with the short id beside it. A subagent call names its parent
 * session and opens the subagent through it. `×N` lists every sample. */
const SessionLink = ({
  detector,
  group,
  titles,
}: {
  readonly detector: string | null;
  readonly group: SessionChipGroup;
  readonly titles: SessionTitles;
}) => {
  const { first: sample, samples } = group;
  const chip = chipLabelOf(sample, titles);
  const subagent = samples.some((s) => s.parent !== null);
  return (
    <span className="flex min-w-0 max-w-full items-center gap-0.5">
      <Button
        asChild
        className="h-6 min-w-0 max-w-full gap-1 px-2 font-mono text-xs"
        size="sm"
        variant="outline"
      >
        <a
          data-testid="stats-session-link"
          href={sampleHash(detector, sample)}
          title={titleOf(samples)}
        >
          <ArrowUpRightIcon className="size-3 shrink-0" />
          <span className="flex min-w-0">
            <span className="truncate">{chip.label}</span>
            {chip.id === null ? null : (
              <span className="shrink-0 text-muted-foreground">
                {" "}
                · {chip.id}
              </span>
            )}
            {subagent ? (
              <span className="shrink-0 text-muted-foreground">
                {" "}
                · subagent
              </span>
            ) : null}
          </span>
        </a>
      </Button>
      {samples.length > 1 ? (
        <Popover>
          <PopoverTrigger asChild>
            <Button
              aria-label={`Open one of ${samples.length} samples`}
              className="h-6 shrink-0 px-1.5 font-mono text-muted-foreground text-xs"
              data-testid="stats-session-more"
              size="sm"
              variant="ghost"
            >
              ×{samples.length}
            </Button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-80 gap-1">
            <SampleTargets detector={detector} samples={samples} />
          </PopoverContent>
        </Popover>
      ) : null}
    </span>
  );
};

/** The links with titles in hand, so a test can render them without a server. */
export const SessionLinksView = ({
  detector,
  samples,
  showLine = true,
  titles,
}: {
  readonly detector: string | null;
  readonly samples: readonly SampleRef[];
  readonly showLine?: boolean;
  readonly titles: SessionTitles;
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
        {sessionChips(samples).map((group) => (
          <SessionLink
            detector={detector}
            group={group}
            key={`${group.first.sid}:${group.first.seq}`}
            titles={titles}
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

/** Sample session links for one row, with the evidence line under them. */
export const SessionLinks = (
  props: Omit<Parameters<typeof SessionLinksView>[0], "titles">
) => <SessionLinksView {...props} titles={useAtomValue(sessionTitlesAtom)} />;
