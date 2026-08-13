/**
 * Redaction for derived strings.
 *
 * `sessions/redact.ts` covers event `title`, `preview` and `body`. The stats
 * service mints strings that never existed as an event field — a command lifted
 * out of a tool-call body, an error signature, a cluster label, a sample line —
 * and those are uncovered. This module closes that hole two ways.
 *
 * First, redaction happens where a string is minted: `extract.ts` builds every
 * fact through a `Redactor`, so a shard on disk holds no unredacted text and the
 * cache can never leak one. Second, anything a detector derives from a fact
 * (labels, notes, cluster names) goes through `ctx.redact` in `finish`, because
 * masking can re-expose a secret the fact carried in a redacted span.
 *
 * Redaction is on by default. `redact: false` is a live re-extract that bypasses
 * the cache entirely, so an unredacted string is never written down.
 */
import { redactText } from "../sessions/redact";
import { clip, collapseWs, SAMPLE_LINE_MAX } from "./rank";

/** Distinct strings memoised per pass. Command text repeats heavily. */
const MEMO_CAP = 4096;

/** The single route every derived string takes before it leaves core. */
export interface Redactor {
  /** False only when the caller explicitly asked for raw text. */
  readonly enabled: boolean;
  /** Redact, collapse whitespace and clip: the form a sample line takes. */
  readonly line: (s: string, max?: number) => string;
  /** Redact when present; `null` passes through as `null`. */
  readonly nullable: (s: string | null | undefined) => string | null;
  /** Redact one derived string, preserving its shape. */
  readonly text: (s: string) => string;
}

/**
 * Build a redactor. Memoised because one pass redacts the same command family
 * thousands of times; the cache is cleared wholesale when full, which changes
 * nothing about the output.
 */
export const makeRedactor = (args: { readonly enabled: boolean }): Redactor => {
  const { enabled } = args;
  const memo = new Map<string, string>();

  const text = (s: string): string => {
    if (!(enabled && s)) {
      return s;
    }
    const hit = memo.get(s);
    if (hit !== undefined) {
      return hit;
    }
    const out = redactText(s);
    if (memo.size >= MEMO_CAP) {
      memo.clear();
    }
    memo.set(s, out);
    return out;
  };

  const line = (s: string, max = SAMPLE_LINE_MAX): string =>
    clip(collapseWs(text(s)), max);

  const nullable = (s: string | null | undefined): string | null =>
    s === null || s === undefined || s === "" ? null : text(s);

  return { enabled, text, line, nullable };
};

/** The redactor used whenever a caller does not pass one. */
export const defaultRedactor = makeRedactor({ enabled: true });
