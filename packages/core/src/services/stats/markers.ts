/**
 * Markers for one session: the same rules the ranked tables use, keyed so a
 * transcript view and the aggregate agree glyph for glyph.
 *
 * Six marks and no more — a named cause, the retry that corrected one, a
 * failure hidden behind a pipe (`piped-exit-code`), any other silent failure,
 * a run that returned at a harness ceiling, and an error signature already
 * seen in this session. A row the fail table suppresses as benign is never
 * marked. A cause mark carries the cause id, the `CauseRow.cause` of its row;
 * a piped mark carries the id its `FailRow.detector` holds; so each links back.
 */
import { CAUSES } from "./detectors/causes";
import { makePairer, type RetryPair } from "./detectors/pairing";
import { callFixOf, FIX_MAX } from "./detectors/retries";
import { asc, ascStr, chain, clip, SAMPLE_LINE_MAX, sortBy } from "./rank";
import type { Redactor } from "./redact";
import {
  type CallFact,
  isBashFact,
  isCallFact,
  type SessionMarker,
  type StatsFact,
} from "./schema";

/** Ids a marker carries besides the cause ids. */
const RETRY = "retry";
const PIPED_EXIT_CODE = "piped-exit-code";
const SILENT_FAIL = "silent-fail";
const CAPPED = "capped";
const REPEAT = "repeat";

/** Chars of an error signature a repeat marker prints. */
const SIG_MAX = 80;

/** Chars of a cause mark: its label and culprit, then the whole fix a row keeps. */
const CAUSE_MARK_MAX = SAMPLE_LINE_MAX + FIX_MAX;

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

/** The named-cause mark: what went wrong, and the fix. */
const causeMarker = (fact: CallFact): SessionMarker | null => {
  if (fact.cause === null) {
    return null;
  }
  const spec = CAUSES[fact.cause];
  const culprit = fact.causeArg === null ? "" : ` (${fact.causeArg})`;
  const fix = callFixOf(fact.cause, fact.causeArg);
  return {
    seq: fact.seq,
    toolUseId: fact.toolUseId,
    detector: fact.cause,
    label:
      fix === null
        ? `${spec.label}${culprit}`
        : `${spec.label}${culprit}: ${fix}`,
  };
};

const SEC_PER_MIN = 60;

/** How long before the retry the failed call ran, as a reader would say it. */
const agoOf = (gapSec: number): string => {
  const sec = Math.round(gapSec);
  return sec < SEC_PER_MIN ? `${sec}s` : `${Math.round(sec / SEC_PER_MIN)}m`;
};

/**
 * The retry mark: this call corrected the failed call it names. The failed
 * call is named by tool and distance in time, which a transcript shows; its
 * internal seq is not.
 */
const retryMarker = (pair: RetryPair): SessionMarker => {
  const what = pair.readBack ? "read-back of" : "retry of";
  const tool = pair.failed.tool;
  const which =
    pair.gapSec === null
      ? `an earlier ${tool} call`
      : `the ${tool} call ${agoOf(pair.gapSec)} earlier`;
  const diff =
    pair.diff === null ? "" : `: ${pair.diff.from} → ${pair.diff.to}`;
  return {
    seq: pair.retry.seq,
    toolUseId: pair.retry.toolUseId,
    detector: RETRY,
    label: `${what} ${which}${diff}`,
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

/**
 * The marks one call carries on its own, redacted. A cause mark keeps its
 * whole fix; the others are one short line.
 */
const callMarks = (fact: CallFact, redact: Redactor): SessionMarker[] => {
  const out: SessionMarker[] = [];
  const cause = causeMarker(fact);
  if (cause !== null) {
    out.push({ ...cause, label: redact.line(cause.label, CAUSE_MARK_MAX) });
  }
  for (const mark of [silentMarker(fact), cappedMarker(fact)]) {
    if (mark !== null) {
      out.push({ ...mark, label: redact.line(mark.label) });
    }
  }
  return out;
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
  const pairer = makePairer((pair) => {
    const mark = retryMarker(pair);
    marks.push({ ...mark, label: redact.line(mark.label) });
  });

  const add = (fact: StatsFact): void => {
    if (!isCallFact(fact) || fact.sid !== sid) {
      return;
    }
    pairer.step(fact);
    if (fact.benign !== null) {
      return;
    }
    marks.push(...callMarks(fact, redact));
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
    finish: () => {
      pairer.flush();
      return sortBy([...marks, ...repeats()], compareMarkers);
    },
  };
};
