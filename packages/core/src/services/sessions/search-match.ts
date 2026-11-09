/** Pure line matcher for transcript search.
 *
 * A line is a candidate when the raw text holds the query (cheap prefilter).
 * Only candidates are JSON-parsed; the match must then sit in a string value
 * that is real content, not a key, an id or a metadata field.
 */

import { escapeRe } from "../stats/detectors/causes";
import { homeCollapser } from "./export/shared";
import { redactText } from "./redact";
import type { SearchHit } from "./search-schema";

/** Keys whose values are metadata or encoded blobs, at any depth. */
const IGNORED_KEYS = new Set([
  "approval_policy",
  "branch",
  "call_id",
  "cli_version",
  "cwd",
  "data",
  "directory",
  "effort",
  "encrypted_content",
  "entrypoint",
  "git",
  "gitBranch",
  "id",
  "kind",
  "leafUuid",
  "messageId",
  "model",
  "model_provider",
  "modelId",
  "originator",
  "parentId",
  "parentUuid",
  "permissionMode",
  "phase",
  "projectId",
  "provider",
  "repository_url",
  "requestId",
  "role",
  "service_tier",
  "sessionId",
  "signature",
  "slug",
  "stop_reason",
  "stop_sequence",
  "stopReason",
  "timestamp",
  "tool_call_id",
  "tool_use_id",
  "toolCallId",
  "type",
  "usage",
  "userType",
  "uuid",
  "version",
]);

/** Record types that hold only run settings, never conversation text. */
const METADATA_RECORDS = new Set(["session_meta", "turn_context"]);

const CONTEXT = 80;
const MAX_DEPTH = 64;
const WHITESPACE = /\s+/g;

/** String values worth searching, in document order. */
const collectText = (value: unknown, out: string[], depth = 0) => {
  if (typeof value === "string") {
    out.push(value);
  } else if (depth < MAX_DEPTH && value !== null && typeof value === "object") {
    if (Array.isArray(value)) {
      for (const v of value) {
        collectText(v, out, depth + 1);
      }
    } else {
      for (const [k, v] of Object.entries(value)) {
        if (!IGNORED_KEYS.has(k)) {
          collectText(v, out, depth + 1);
        }
      }
    }
  }
};

const words = (q: string) => q.split(WHITESPACE).filter((w) => w !== "");

/**
 * Case-insensitive patterns for `q`. Whitespace runs in `q` match any
 * whitespace run. `content` runs on decoded text. `raw` runs on a JSON line
 * and accepts both the plain and the JSON-escaped form of each word, so it
 * never rejects a line that `content` would accept.
 */
export const queryPatterns = (q: string) => {
  const parts = words(q);
  return {
    content: new RegExp(parts.map(escapeRe).join("\\s+"), "i"),
    raw: new RegExp(
      parts
        .map((w) => {
          const plain = escapeRe(w);
          const escaped = escapeRe(JSON.stringify(w).slice(1, -1));
          return escaped === plain ? plain : `(?:${plain}|${escaped})`;
        })
        .join("(?:\\s|\\\\[nrtf]|\\\\u000[bB])+"),
      "i"
    ),
  };
};

const isMetadataRecord = (record: unknown) =>
  typeof record === "object" &&
  record !== null &&
  METADATA_RECORDS.has((record as { type?: unknown }).type as string);

/** The searchable text of one JSONL line, or undefined if it is not JSON. */
const contentOf = (line: string) => {
  try {
    const record: unknown = JSON.parse(line);
    if (isMetadataRecord(record)) {
      return;
    }
    const parts: string[] = [];
    collectText(record, parts);
    return parts.join("\n");
  } catch {
    return;
  }
};

// Built from strings so the control bytes stay `\u` escapes in source.
const re = (pattern: string) => new RegExp(pattern, "g");
const ANSI_OSC = re("\\u001b\\][^\\u0007\\u001b]*(?:\\u0007|\\u001b\\\\)?");
const ANSI_CSI = re("\\u001b\\[[0-9;?]*[ -/]*[@-~]?");
const ANSI_ESC = re("\\u001b.?");
/** C0, DEL, C1 and the bidi marks that can disguise text when printed. */
const CONTROL = re(
  "[\\u0000-\\u001f\\u007f-\\u009f\\u200e\\u200f\\u202a-\\u202e\\u2066-\\u2069]"
);

/** Drop terminal escape sequences and fold whitespace, so a snippet is safe to print. */
const collapse = (s: string) =>
  s
    .replace(ANSI_OSC, " ")
    .replace(ANSI_CSI, " ")
    .replace(ANSI_ESC, " ")
    .replace(CONTROL, " ")
    .replace(WHITESPACE, " ")
    .trim();

const HIGH_SURROGATE_MIN = 0xd8_00;
const LOW_SURROGATE_MIN = 0xdc_00;
const LOW_SURROGATE_MAX = 0xdf_ff;
const isHighSurrogate = (code: number) =>
  code >= HIGH_SURROGATE_MIN && code < LOW_SURROGATE_MIN;
const isLowSurrogate = (code: number) =>
  code >= LOW_SURROGATE_MIN && code <= LOW_SURROGATE_MAX;

/** Cut `s` to `CONTEXT` chars each side of `[at, at + len)`, with ellipses. */
const cut = (s: string, at: number, len: number) => {
  let from = Math.max(0, at - CONTEXT);
  let to = Math.min(s.length, at + len + CONTEXT);
  if (from > 0 && isLowSurrogate(s.charCodeAt(from))) {
    from += 1;
  }
  if (to < s.length && isHighSurrogate(s.charCodeAt(to - 1))) {
    to -= 1;
  }
  const lead = from > 0 ? "…" : "";
  return {
    text: `${lead}${s.slice(from, to)}${to < s.length ? "…" : ""}`,
    start: at - from + lead.length,
  };
};

/** Redact secrets and home paths in free text. */
export const redactDisplay = (
  s: string,
  collapsePaths: (text: string) => string = homeCollapser()
) => collapsePaths(redactText(s));

/** Snippet around the first match of `pattern` in `text`, redacted when asked. */
export const buildSnippet = (args: {
  readonly text: string;
  readonly pattern: RegExp;
  readonly redact: boolean;
  readonly collapsePaths?: ((text: string) => string) | undefined;
}): Pick<SearchHit, "snippet" | "matchStart" | "matchEnd"> => {
  const { text, pattern, redact, collapsePaths } = args;
  const window = collapse(redact ? redactDisplay(text, collapsePaths) : text);
  const found = pattern.exec(window);
  if (found === null) {
    const mid = Math.floor(window.length / 2);
    return { snippet: cut(window, mid, 0).text };
  }
  const { text: snippet, start } = cut(window, found.index, found[0].length);
  return { snippet, matchStart: start, matchEnd: start + found[0].length };
};

interface LineMatches {
  readonly hits: readonly SearchHit[];
  /** Every matching line, including those not kept as hits. */
  readonly matchCount: number;
}

/**
 * Matching lines of a transcript. Hits are distinct by snippet (agents repeat
 * the same text in several records) and capped at `maxHits`; `matchCount`
 * counts every matching line. A blank `q` matches nothing.
 */
export const matchTranscript = (args: {
  readonly text: string;
  readonly q: string;
  readonly redact: boolean;
  readonly maxHits: number;
  readonly collapsePaths?: ((text: string) => string) | undefined;
}): LineMatches => {
  const { text, q, redact, maxHits, collapsePaths } = args;
  const none = { hits: [], matchCount: 0 } satisfies LineMatches;
  if (words(q).length === 0) {
    return none;
  }
  const { content: pattern, raw } = queryPatterns(q);
  if (!raw.test(text)) {
    return none;
  }
  const hits: SearchHit[] = [];
  const seen = new Set<string>();
  let matchCount = 0;
  let line = 0;
  for (const row of text.split("\n")) {
    line += 1;
    if (!raw.test(row)) {
      continue;
    }
    const content = contentOf(row);
    const found = content === undefined ? null : pattern.exec(content);
    if (content === undefined || found === null) {
      continue;
    }
    matchCount += 1;
    if (hits.length >= maxHits) {
      continue;
    }
    const snippet = buildSnippet({
      text: content,
      pattern,
      redact,
      collapsePaths,
    });
    if (!seen.has(snippet.snippet)) {
      seen.add(snippet.snippet);
      hits.push({ line, ...snippet });
    }
  }
  return { hits, matchCount };
};
