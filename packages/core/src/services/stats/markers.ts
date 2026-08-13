/**
 * Markers for one session: the same rules the ranked tables use, keyed so a
 * transcript view and the aggregate agree glyph for glyph.
 *
 * Four marks and no more — a named rule, a silent failure, a run that returned
 * at a harness ceiling, and an error signature already seen in this session. A
 * row the fail table suppresses as benign is never marked. Every `detector` id
 * here is the id a `FailRow` carries, so a marker links back to its row.
 */
import { isZshEqualsAbort } from "./detectors/failures";
import { asc, ascStr, chain, clip, sortBy } from "./rank";
import type { Redactor } from "./redact";
import {
  type CallFact,
  isBashFact,
  isCallFact,
  type SessionMarker,
  type StatsFact,
} from "./schema";

/** Ids a marker carries. The first two are also `FailRow.detector` values. */
const ZSH_EQUALS_ABORT = "zsh-equals-abort";
const PIPED_EXIT_CODE = "piped-exit-code";
const SILENT_FAIL = "silent-fail";
const CAPPED = "capped";
const REPEAT = "repeat";

/** Chars of an error signature a repeat marker prints. */
const SIG_MAX = 80;

/** A repeat needs a second occurrence. */
const MIN_REPEATS = 2;

/** Markers read in transcript order; the id breaks a tie on one call. */
const compareMarkers = chain<SessionMarker>(
  asc((m) => m.seq),
  ascStr((m) => m.detector)
);

/** What a call collects markers into. */
export interface MarkerCollector {
  /** Fold one fact. Facts from other sessions are ignored. */
  readonly add: (fact: StatsFact) => void;
  /** Every marker for the session, in transcript order. */
  readonly finish: () => readonly SessionMarker[];
}

/** The named-rule mark: a bare `echo ===` separator zsh never ran past. */
const ruleMarker = (fact: CallFact): SessionMarker | null => {
  if (!(isBashFact(fact) && fact.failed && isZshEqualsAbort(fact.cmd))) {
    return null;
  }
  return {
    seq: fact.seq,
    toolUseId: fact.toolUseId,
    detector: ZSH_EQUALS_ABORT,
    label:
      'unquoted echo === separator: the rest of the line never ran — quote it as echo "==="',
  };
};

/** The silent-failure mark. It says what was printed, never "this broke". */
const silentMarker = (fact: CallFact): SessionMarker | null => {
  if (fact.err || !fact.failed || fact.failReason === null) {
    return null;
  }
  const piped = isBashFact(fact) && fact.piped;
  return {
    seq: fact.seq,
    toolUseId: fact.toolUseId,
    detector: piped ? PIPED_EXIT_CODE : SILENT_FAIL,
    label: piped
      ? `printed a failure (${fact.failReason}) and exited 0: the pipe owns the exit code`
      : `printed a failure (${fact.failReason}) and exited 0`,
  };
};

/** The ceiling mark: the duration measures the cap, not the command. */
const cappedMarker = (fact: CallFact): SessionMarker | null => {
  if (!(isBashFact(fact) && fact.capped)) {
    return null;
  }
  return {
    seq: fact.seq,
    toolUseId: fact.toolUseId,
    detector: CAPPED,
    label: "returned at a harness ceiling: the duration is a floor",
  };
};

/** Collect every marker for one session id. */
export const makeMarkerCollector = (args: {
  readonly redact: Redactor;
  readonly sid: string;
}): MarkerCollector => {
  const { redact, sid } = args;
  const marks: SessionMarker[] = [];
  /** Calls per error signature, in emit order, for the repeat mark. */
  const bySig = new Map<string, CallFact[]>();

  const add = (fact: StatsFact): void => {
    if (!isCallFact(fact) || fact.sid !== sid || fact.benign !== null) {
      return;
    }
    for (const mark of [
      ruleMarker(fact),
      silentMarker(fact),
      cappedMarker(fact),
    ]) {
      if (mark !== null) {
        marks.push({ ...mark, label: redact.line(mark.label) });
      }
    }
    if (fact.sig !== null && (fact.failed || fact.err)) {
      const seen = bySig.get(fact.sig);
      if (seen) {
        seen.push(fact);
      } else {
        bySig.set(fact.sig, [fact]);
      }
    }
  };

  const repeats = (): readonly SessionMarker[] =>
    [...bySig.entries()].flatMap(([sig, facts]) => {
      if (facts.length < MIN_REPEATS) {
        return [];
      }
      const label = redact.line(clip(sig, SIG_MAX));
      return facts.slice(1).map((fact, index) => ({
        seq: fact.seq,
        toolUseId: fact.toolUseId,
        detector: REPEAT,
        label: `repeat ${index + MIN_REPEATS} of ${facts.length} in this session: ${label}`,
      }));
    });

  return {
    add,
    finish: () => sortBy([...marks, ...repeats()], compareMarkers),
  };
};
