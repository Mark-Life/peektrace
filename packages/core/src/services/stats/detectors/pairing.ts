/**
 * Retry pairs: a failed call followed, within the next few calls of the same
 * thread, by a similar call of the same tool that succeeded. The pair is the
 * wasted round trip a rule or an env fix would have saved.
 *
 * The pass is sequence-aware and bounded. Facts of one transcript arrive
 * together and in `seq` order, so the state is one lane per (session, side) —
 * a main transcript interleaves its sidechain calls under one `sid` — holding
 * at most the seeds of the last `RETRY_WINDOW` calls. A new session drops
 * every lane.
 *
 * Gates, measured on a hand-labelled sample: same tool (a missing binary may
 * be retried by any shell call), not a parallel sibling of the same model call,
 * not itself failed, hybrid similarity >= 0.6, and at most ten minutes later.
 * A seed with no named cause is work iteration until shown otherwise, so it
 * never seeds from a compiler or test signature and loses its pair when an
 * edit, by an edit tool or a shell write such as `sed -i`, sits between the
 * two calls: tsc fails, the agent edits, tsc passes. A call with no text to
 * compare seeds only for a cause whose retry is any call of its tool.
 *
 * Chains are claimed once. Fail A, fail A', pass A'' is one pair (A' → A'')
 * and two wasted calls. A success is claimed by the latest similar seed, and a
 * seed keeps the best-scoring success in its window, earliest on a tie.
 */
import {
  type BashFact,
  type CallFact,
  type CauseId,
  isBashFact,
  isCallFact,
  type StatsFact,
} from "../schema";
import { binName, commandIndex, shellContexts } from "../shell-lex";
import { CAUSES, FILE_EDIT_TOOLS } from "./causes";
import { USER_REJECTED } from "./failures";
import { MAX_RETRY_GAP_SEC, PAIR_THRESHOLD, RETRY_WINDOW } from "./retry-gates";
import {
  type FocusMode,
  hybridSimilarity,
  ratio,
  type SimilarityInput,
  segments,
} from "./similarity";
import { waitShape } from "./waiting";

/** Chars of a learned diff side. */
const DIFF_MAX = 80;

const MS_PER_SEC = 1000;

/** The token change a retry made. */
export interface LearnedDiff {
  readonly from: string;
  readonly to: string;
}

/** One failed call and the call that corrected it. */
export interface RetryPair {
  /** The seed's cause; `unclassified` when no detector named one. */
  readonly cause: CauseId;
  readonly diff: LearnedDiff | null;
  readonly failed: CallFact;
  /** Call-to-call seconds, or null when either call has no time. */
  readonly gapSec: number | null;
  /** True for a read-back of persisted output, not a corrected retry. */
  readonly readBack: boolean;
  readonly retry: CallFact;
}

/** What opens a seed: a failing cause (extract marks its call failed), or an
 * unexplained error. */
const seedCauseOf = (fact: CallFact): CauseId | null => {
  if (fact.cause !== null) {
    return CAUSES[fact.cause].failing ? fact.cause : null;
  }
  if (!fact.err || fact.benign !== null || fact.failReason !== null) {
    return null;
  }
  return fact.sig !== null && USER_REJECTED.test(fact.sig)
    ? null
    : "unclassified";
};

/**
 * The cause a call counts as a hit of: a failing cause or an unexplained
 * error, as for a seed, or a persisted output, which is waste, not failure.
 */
export const hitCauseOf = (fact: CallFact): CauseId | null =>
  fact.cause === "output-too-large" ? fact.cause : seedCauseOf(fact);

const modeOf = (cause: CauseId): FocusMode => {
  switch (cause) {
    case "zsh-glob-flag":
    case "zsh-glob-path":
      return "glob";
    case "command-not-found":
      return "cnf";
    case "sleep-blocked":
      return "sleep";
    default:
      return "whole";
  }
};

/** Causes whose retry is the next call of the tool, whatever it carries: a
 * StructuredOutput call has no argument to compare. */
const ANY_RETRY: ReadonlySet<CauseId> = new Set(["structured-output-schema"]);

/** The text a call is compared on: the command, else the primary argument. */
const textOf = (fact: CallFact): string =>
  isBashFact(fact) ? fact.cmd : (fact.arg ?? "");

const tsMs = (fact: CallFact): number | null => {
  if (fact.ts === null) {
    return null;
  }
  const t = Date.parse(fact.ts);
  return Number.isNaN(t) ? null : t;
};

/** True when two calls came from the same model call: parallel, not a retry. */
const areSiblings = (a: CallFact, b: CallFact): boolean => {
  if (a.req !== null && b.req !== null) {
    return a.req === b.req;
  }
  return a.ts !== null && a.ts === b.ts;
};

const WS = /\s/;

/** Chain and pipe operators end a word outside quotes; they are not words. */
const OPERATOR_CHAR = /[;&|]/;

/** The mark `clip` leaves where it cut a string. */
const CLIP_MARK = "…";

/**
 * Words of a command with their quotes kept, so `*.ts` → `'*.ts'` shows. A
 * word ends at whitespace or an operator only where the shell reads code: not
 * inside quotes, not after a backslash, not in a `$(...)` inside quotes.
 */
const quotedWords = (cmd: string): readonly string[] => {
  const ctx = shellContexts(cmd);
  const words: string[] = [];
  let buf = "";
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i] ?? "";
    const splits = ctx[i] === "code" || ctx[i] === "heredoc";
    if (splits && (WS.test(c) || OPERATOR_CHAR.test(c))) {
      if (buf) {
        words.push(buf);
      }
      buf = "";
    } else {
      buf += c;
    }
  }
  if (buf) {
    words.push(buf);
  }
  return words;
};

/**
 * Words of a command as stored. A command over the fact's cap ends in `…`
 * mid-word; that word is the cut, not something the agent typed, so it is
 * dropped rather than read as a change.
 */
const unclippedWords = (cmd: string): readonly string[] => {
  const words = quotedWords(cmd);
  return cmd.endsWith(CLIP_MARK) ? words.slice(0, -1) : words;
};

/** Suffix LCS lengths: `t[i][j]` is the LCS of `a[i:]` and `b[j:]`. */
const lcsTable = (
  a: readonly string[],
  b: readonly string[]
): readonly (readonly number[])[] => {
  const t: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0)
  );
  for (let i = a.length - 1; i >= 0; i--) {
    const row = t[i] ?? [];
    const below = t[i + 1] ?? [];
    for (let j = b.length - 1; j >= 0; j--) {
      row[j] =
        a[i] === b[j]
          ? (below[j + 1] ?? 0) + 1
          : Math.max(below[j] ?? 0, row[j + 1] ?? 0);
    }
  }
  return t;
};

/** Words of `a` and of `b` that the longest common subsequence leaves out. */
const wordDiff = (
  a: readonly string[],
  b: readonly string[]
): { readonly removed: string[]; readonly added: string[] } => {
  const t = lcsTable(a, b);
  const removed: string[] = [];
  const added: string[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      i += 1;
      j += 1;
    } else if ((t[i + 1]?.[j] ?? 0) >= (t[i]?.[j + 1] ?? 0)) {
      removed.push(a[i] ?? "");
      i += 1;
    } else {
      added.push(b[j] ?? "");
      j += 1;
    }
  }
  removed.push(...a.slice(i));
  added.push(...b.slice(j));
  return { removed, added };
};

const clipDiff = (s: string): string =>
  s.length > DIFF_MAX ? `${s.slice(0, DIFF_MAX)}${CLIP_MARK}` : s;

/**
 * A diff as it ships, both sides clipped; null when they read the same: a
 * binary retried by name after a PATH fix, or two long words that differ only
 * past the clip.
 */
const diffOfSides = (from: string, to: string): LearnedDiff | null => {
  const a = clipDiff(from);
  const b = clipDiff(to);
  return a === b ? null : { from: a, to: b };
};

/** Similarity an added word needs to read as the rewrite of a removed one. */
const REWRITE_MIN = 0.5;

/** The added word most like `from`, or null when none reads as its rewrite. */
const rewriteOf = (
  from: string,
  added: readonly string[]
): { readonly to: string; readonly score: number } | null => {
  let found: { readonly to: string; readonly score: number } | null = null;
  for (const w of added) {
    const score = ratio(from, w);
    if (score >= REWRITE_MIN && (found === null || score > found.score)) {
      found = { to: w, score };
    }
  }
  return found;
};

/**
 * The single token change that best explains a corrected shell retry. With a
 * culprit: the removed word naming it, and the added word most like it, or
 * `(dropped)` when none is; null when no removed word names it. Without one:
 * the removed and added pair most alike.
 * A word on both sides only moved, so it is never a change.
 */
const learnedDiff = (args: {
  readonly failedCmd: string;
  readonly retryCmd: string;
  /** Picks the removed word that names the culprit. */
  readonly culprit: ((word: string) => boolean) | null;
}): LearnedDiff | null => {
  const diff = wordDiff(
    unclippedWords(args.failedCmd),
    unclippedWords(args.retryCmd)
  );
  const moved = new Set(diff.removed.filter((w) => diff.added.includes(w)));
  const removed = diff.removed.filter((w) => !moved.has(w));
  const added = diff.added.filter((w) => !moved.has(w));
  const culprit =
    args.culprit === null ? undefined : removed.find(args.culprit);
  if (args.culprit !== null) {
    // A culprit missing from the removed words (the stored command is clipped
    // before it) has no change to name; an unrelated swap would mislead.
    return culprit === undefined
      ? null
      : diffOfSides(culprit, rewriteOf(culprit, added)?.to ?? "(dropped)");
  }
  let best: {
    readonly from: string;
    readonly to: string;
    score: number;
  } | null = null;
  for (const from of removed) {
    const r = rewriteOf(from, added);
    if (r !== null && (best === null || r.score > best.score)) {
      best = { from, to: r.to, score: r.score };
    }
  }
  return best === null ? null : diffOfSides(best.from, best.to);
};

const EQUALS_WORD = /^["']?=/;

/** How a cause's culprit word is found among the removed words. */
const culpritOf = (
  cause: CauseId,
  arg: string | null
): ((word: string) => boolean) | null => {
  if (cause === "zsh-equals-abort") {
    return (w) => EQUALS_WORD.test(w);
  }
  if (cause === "pkill-self-kill") {
    return (w) => w === "pkill" || w === "pgrep" || w === "kill";
  }
  return arg === null ? null : (w) => w.includes(arg);
};

/** A failed call waiting for its retry. */
interface Seed {
  best: {
    readonly retry: CallFact;
    readonly score: number;
    readonly segment: string;
  } | null;
  readonly cause: CauseId;
  editSeen: boolean;
  readonly fact: CallFact;
  readonly input: SimilarityInput;
  /** Persisted output: waits for a call that reads the saved file back. */
  readonly readBack: boolean;
  seen: number;
}

/** Subshell, group and substitution openers glued to a command word. */
const OPENER = /^(?:\$\(|[({`])+/;

/** Shell keywords that only lead into the command. */
const LEAD_WORDS = new Set(["!", "do", "else", "if", "then", "until", "while"]);

/** Builtins: a segment that starts with one runs no binary to learn from. */
const BUILTINS = new Set([
  ".",
  "alias",
  "cd",
  "eval",
  "exit",
  "export",
  "false",
  "hash",
  "popd",
  "pushd",
  "rehash",
  "return",
  "set",
  "source",
  "true",
  "unset",
]);

/**
 * The binary a segment runs, by name: its first word past openers, lead-in
 * keywords, env assignments and the lexer's wrappers (`timeout 30 cp` runs
 * `cp`; `/usr/bin/rsync` is `rsync`). Null when that word is a builtin or
 * there is none.
 */
const commandWordOf = (s: string): string | null => {
  const words = s
    .split(" ")
    .map((raw) => raw.replace(OPENER, ""))
    .filter((w) => w !== "");
  const lead = words.findIndex((w) => !LEAD_WORDS.has(w));
  const rest = lead < 0 ? [] : words.slice(lead);
  const word = rest[commandIndex(rest)];
  if (word === undefined) {
    return null;
  }
  const name = binName(word);
  return BUILTINS.has(name) ? null : name;
};

const SLEEP_WORD = /\bsleep\s+\.?\d+(?:\.\d+)?/;

/** The script of a `bash -c '…'` (or `sh`, `zsh`), which `waitShape` masks. */
const SHELL_PAYLOAD = /\b(?:ba|z)?sh\s+-\w*c\s+(['"])([\s\S]*?)\1/g;

/** The wait shape of a call, looking inside a quoted shell script too: a
 * `timeout 100 bash -c 'until …; do sleep 3; done'` polls. */
const waitIn = (retry: BashFact): BashFact["wait"] => {
  if (retry.wait !== null) {
    return retry.wait;
  }
  for (const m of retry.cmd.matchAll(SHELL_PAYLOAD)) {
    const inner = waitShape(m[2] ?? "");
    if (inner !== null) {
      return inner;
    }
  }
  return null;
};

/** How the retry of a blocked `sleep N` waited instead. */
const waitOf = (retry: BashFact): string => {
  if (retry.bg) {
    return "run_in_background";
  }
  switch (waitIn(retry)) {
    case "loop-poll":
      return "poll loop";
    case "watcher":
      return "watcher";
    case "curl-retry":
      return "curl --retry";
    case "bare-delay":
      return SLEEP_WORD.exec(retry.cmd)?.[0] ?? "sleep";
    default:
      return "(no wait)";
  }
};

/** The diff a closed pair carries, by how its seed was cut. */
const diffOf = (
  seed: Seed,
  retry: CallFact,
  segment: string
): LearnedDiff | null => {
  if (!(isBashFact(seed.fact) && isBashFact(retry))) {
    return null;
  }
  const arg = seed.fact.causeArg;
  if (
    arg === null &&
    (seed.cause === "command-not-found" || seed.cause === "sleep-blocked")
  ) {
    return null;
  }
  if (seed.cause === "sleep-blocked") {
    return diffOfSides(arg ?? "", waitOf(retry));
  }
  if (seed.cause === "command-not-found") {
    // Only a focused seed scored segment by segment: in whole-line mode the
    // segment is the whole retry, whose first word is usually `cd`.
    const ran = seed.input.focus === null ? null : commandWordOf(segment);
    return ran === null ? null : diffOfSides(arg ?? "", ran);
  }
  return learnedDiff({
    failedCmd: seed.fact.cmd,
    retryCmd: retry.cmd,
    culprit: culpritOf(seed.cause, arg),
  });
};

const gapOf = (a: CallFact, b: CallFact): number | null => {
  const ta = tsMs(a);
  const tb = tsMs(b);
  return ta === null || tb === null ? null : (tb - ta) / MS_PER_SEC;
};

/**
 * Shell commands that write a file: an in-place sed or perl, a script that
 * opens a file for writing, and `cat > f` or `tee f` (a heredoc too).
 * `/dev/...` targets are not files.
 */
const SHELL_EDITS: readonly RegExp[] = [
  /\b(?:g?sed|perl)\s(?:[^|;&\n]*\s)?(?:-[A-Za-z]*i|--in-place)\b/,
  /\bopen\([^)]*,\s*(?:mode\s*=\s*)?["'][wax]/,
  /\.write_(?:text|bytes)\(|\bwriteFile(?:Sync)?\(|\bBun\.write\(/,
  /\bcat\b[^|;&\n]*>\s*(?!\/dev\/)[^\s&>]/,
  /\btee\s+(?:-a\s+)?(?!\/dev\/|-)\S/,
];

/** Tools whose primary argument is a shell line, though not a shell fact. */
const SHELL_ARG_TOOLS: ReadonlySet<string> = new Set(["exec_command"]);

/** The shell line a call ran, or null when it ran none. */
const shellLineOf = (fact: CallFact): string | null => {
  if (isBashFact(fact)) {
    return fact.cmd;
  }
  return SHELL_ARG_TOOLS.has(fact.tool) ? fact.arg : null;
};

/**
 * A file edit: by tool, by an older Codex's `apply_patch` shell call, or by a
 * shell command that writes a file.
 */
const editsFile = (fact: CallFact): boolean => {
  if (
    FILE_EDIT_TOOLS.has(fact.tool) ||
    (isBashFact(fact) && fact.bin === "apply_patch")
  ) {
    return true;
  }
  const line = shellLineOf(fact);
  return line !== null && SHELL_EDITS.some((re) => re.test(line));
};

/** True when `retry` is in reach of `seed` on every gate but similarity. */
const inReach = (seed: Seed, retry: CallFact): boolean => {
  const sameTool =
    retry.tool === seed.fact.tool ||
    (seed.cause === "command-not-found" && isBashFact(retry));
  if (!sameTool || areSiblings(seed.fact, retry)) {
    return false;
  }
  const gap = gapOf(seed.fact, retry);
  if (gap !== null && (gap < 0 || gap > MAX_RETRY_GAP_SEC)) {
    return false;
  }
  return !(seed.cause === "unclassified" && seed.editSeen);
};

/** True when a call reads back the file a persisted output was saved to. */
const readsBack = (seed: Seed, fact: CallFact): boolean => {
  const name = seed.fact.causeArg;
  if (name === null || fact === seed.fact || areSiblings(seed.fact, fact)) {
    return false;
  }
  return (
    (fact.arg?.includes(name) ?? false) ||
    (isBashFact(fact) && fact.cmd.includes(name))
  );
};

/** Folds facts into retry pairs, one session at a time. */
export interface Pairer {
  /** Close every open seed, emitting the pairs they hold. */
  readonly flush: () => void;
  /** Fold one fact. Facts of one session must arrive together, in seq order. */
  readonly step: (fact: StatsFact) => void;
}

/** Report a seed's best retry, if it found one. */
const closeSeed = (seed: Seed, onPair: (pair: RetryPair) => void): void => {
  if (seed.best === null) {
    return;
  }
  const { retry, segment } = seed.best;
  onPair({
    cause: seed.cause,
    failed: seed.fact,
    retry,
    gapSec: gapOf(seed.fact, retry),
    readBack: seed.readBack,
    diff: seed.readBack ? null : diffOf(seed, retry, segment),
  });
};

/** What one call does to one open seed. */
const verdictOf = (
  seed: Seed,
  fact: CallFact,
  split: (cmd: string) => readonly string[]
): "skip" | "chain" | { readonly score: number; readonly segment: string } => {
  if (!inReach(seed, fact)) {
    return "skip";
  }
  const sim = hybridSimilarity(seed.input, textOf(fact), PAIR_THRESHOLD, split);
  if (sim.score < PAIR_THRESHOLD) {
    return "skip";
  }
  // A similar call that failed again takes the chain over: the older seed
  // closes as a wasted call and the newer one is the seed now.
  return fact.failed || fact.err ? "chain" : sim;
};

/** A seed's verdict on a call that scored as its retry. */
interface Claim {
  readonly score: number;
  readonly seed: Seed;
  readonly segment: string;
}

/** Credit a call to the persisted-output seeds that wait for its read-back. */
const offerReadBack = (seeds: readonly Seed[], fact: CallFact): void => {
  for (const seed of seeds) {
    if (seed.readBack && seed.best === null && readsBack(seed, fact)) {
      seed.best = { retry: fact, score: 1, segment: "" };
    }
  }
};

/**
 * Offer one call to the open seeds of its lane; the seeds a newer failure took
 * over are returned to be closed. A corrected call is one retry: it goes to
 * the newest similar seed still without one, else to the newest it improves,
 * so parallel failed siblings followed by parallel fixes each keep a pair.
 */
const offerTo = (seeds: readonly Seed[], fact: CallFact): Set<Seed> => {
  offerReadBack(seeds, fact);
  const closed = new Set<Seed>();
  // One call is scored against several seeds: split its text once.
  let segs: readonly string[] | null = null;
  const split = (cmd: string): readonly string[] => {
    segs ??= segments(cmd);
    return segs;
  };
  let open: Claim | null = null;
  let better: Claim | null = null;
  for (const seed of [...seeds].reverse()) {
    if (seed.readBack || open !== null) {
      continue;
    }
    const v = verdictOf(seed, fact, split);
    if (v === "chain") {
      closed.add(seed);
    } else if (v === "skip") {
      // Not this seed's retry.
    } else if (seed.best === null) {
      open = { seed, ...v };
    } else if (better === null && v.score > seed.best.score) {
      better = { seed, ...v };
    }
  }
  const pick = open ?? better;
  if (pick !== null) {
    pick.seed.best = { retry: fact, score: pick.score, segment: pick.segment };
  }
  return closed;
};

/** The seed a call opens, or null. A persisted output with no saved file
 * name has nothing to be read back by. */
const seedOf = (fact: CallFact): Seed | null => {
  const cause = hitCauseOf(fact);
  const readBack = cause === "output-too-large";
  if (cause === null || (readBack && fact.causeArg === null)) {
    return null;
  }
  // An empty text scores 1 against any call: only a cause whose retry is any
  // call of its tool may seed without one.
  const text = textOf(fact);
  if (text === "" && !(readBack || ANY_RETRY.has(cause))) {
    return null;
  }
  return {
    fact,
    cause,
    readBack,
    input: {
      text,
      focus: fact.focus,
      mode: modeOf(cause),
      shell: isBashFact(fact),
    },
    best: null,
    editSeen: false,
    seen: 0,
  };
};

/** Build a pairer that reports each closed pair to `onPair`. */
export const makePairer = (onPair: (pair: RetryPair) => void): Pairer => {
  let sid: string | null = null;
  const lanes = new Map<string, Seed[]>();

  const flush = (): void => {
    for (const seeds of lanes.values()) {
      for (const seed of seeds) {
        closeSeed(seed, onPair);
      }
    }
    lanes.clear();
  };

  const step = (fact: StatsFact): void => {
    if (!isCallFact(fact)) {
      return;
    }
    if (fact.sid !== sid) {
      flush();
      sid = fact.sid;
    }
    const key = fact.side ? "side" : "main";
    const seeds = lanes.get(key) ?? [];
    const closed = offerTo(seeds, fact);
    const edit = editsFile(fact);
    const kept: Seed[] = [];
    for (const seed of seeds) {
      // A parallel sibling of the failed call is never its retry, so it does
      // not use up the window either.
      if (!areSiblings(seed.fact, fact)) {
        seed.seen += 1;
      }
      seed.editSeen ||= edit;
      if (closed.has(seed) || seed.seen >= RETRY_WINDOW) {
        closeSeed(seed, onPair);
      } else {
        kept.push(seed);
      }
    }
    const fresh = seedOf(fact);
    if (fresh !== null) {
      kept.push(fresh);
    }
    lanes.set(key, kept);
  };

  return { step, flush };
};
