/**
 * The cause catalog: named reasons a call failed, each with a detector over the
 * result text, a kind saying where the fix lives, and the one-line fix itself.
 *
 * Detection runs at extract time while the raw output and command are in hand,
 * including exit-0 rows: a zsh glob abort in a chain segment, or on the left of
 * a pipe, never reaches the exit code. The fact stores the cause id, the
 * culprit and the failing segment, never the output.
 *
 * Grepping transcripts prints these same messages, so every detector is
 * anchored. A shell message must start a line with the shell's own prefix and
 * name a culprit that is a word of the command that ran; a harness message
 * must open the result body. A `jq` over a transcript then quotes
 * `(eval):1: no matches found: --include=*.ts` without being one.
 */
import { collapseWs } from "../rank";
import type { CauseId, CauseKind } from "../schema";
import {
  UNARY_WRAPPERS,
  unescaped,
  unquotedAt,
  WRAPPER_OPERAND,
} from "../shell-lex";
import { FOCUS_SEP, normSeg, segments } from "./similarity";

/** One keyword probe: every regex must match a memory line or its ±1 window. */
export type RuleProbe = readonly RegExp[];

/** One catalog entry, keyed by its `CauseId` in `CAUSES`. */
export interface CauseSpec {
  /**
   * The env or prompt-level change to make when the rule is already in a
   * memory file and the cause still fires. Null for a cause with no rule.
   */
  readonly advice: string | null;
  /** False when the cause is waste, not failure: the call worked. */
  readonly failing: boolean;
  /** The fix to state when no row-level detail sharpens it. */
  readonly fix: string | null;
  readonly kind: CauseKind;
  readonly label: string;
  /**
   * Denominator for per-1k rates: shell calls, calls of these tools, or calls
   * of the one tool the row groups.
   */
  readonly per: "shell" | "row-tool" | readonly string[];
  /**
   * Keyword probes that read a memory line as "this rule is already stated".
   * Empty when the rule is never checked; `command-not-found` builds its own
   * from the binaries a row names.
   */
  readonly probes: readonly RuleProbe[];
}

const WRITE_TOOL_NAMES: readonly string[] = [
  "Write",
  "Edit",
  "MultiEdit",
  "NotebookEdit",
];

/** Claude's tools that write a file: the harness's read-first checks. */
export const WRITE_TOOLS: ReadonlySet<string> = new Set(WRITE_TOOL_NAMES);

/**
 * Tools that edit a file, across the four agents: Claude's, Pi's and
 * OpenCode's lowercase `edit`/`write` (and OpenCode's `multiedit`/`patch`),
 * and Codex's `apply_patch`. An edit between a failure and its retry makes
 * the retry a fix-and-rerun, not a repeat.
 */
export const FILE_EDIT_TOOLS: ReadonlySet<string> = new Set([
  ...WRITE_TOOL_NAMES,
  "edit",
  "write",
  "multiedit",
  "patch",
  "apply_patch",
]);

/**
 * Where a PATH fix reaches the agent's shell. Every zsh reads ~/.zshenv.
 * Claude Code snapshots the user's rc file, so ~/.bashrc reaches its bash; a
 * plain non-interactive bash reads only the file $BASH_ENV names, unset by
 * default, so PATH has to be in the environment the agent starts with.
 */
export const PATH_FILES =
  "~/.zshenv for zsh (every zsh reads it); ~/.bashrc for bash under Claude Code, whose shell snapshot captures it; other agents' bash reads no rc file, so export PATH before starting them";

/** Where a zsh option belongs: ~/.zshenv is read by every zsh, interactive or
 * not, and Claude Code's shell snapshot still captures it. */
const ZSHENV = "~/.zshenv (every zsh reads it)";

/** A shell glob word, not Claude's `Glob` tool. */
const SHELL_GLOB = /\bglob(?:s|bing)?\b(?!\s+tool)/i;

/**
 * The output-too-large fix for a row whose hits are mostly `git diff`, and the
 * probe that reads a memory line as that rule. Case-sensitive: `HEAD` in a
 * `git diff HEAD` line is not `| head`.
 */
export const GIT_DIFF_OUTPUT: {
  readonly fix: string;
  readonly probes: readonly RuleProbe[];
} = {
  fix: "Never print a full-branch git diff: run git diff --stat first, then diff one file at a time.",
  probes: [[/\bgit diff\b/, /--stat\b|\bone file\b|\|\s*head\b/]],
};

/** Every cause, in detection priority order. */
export const CAUSES: Readonly<Record<CauseId, CauseSpec>> = {
  "zsh-glob-flag": {
    kind: "rule",
    label: "unquoted glob in a flag value (zsh no matches found)",
    fix: "Shell is zsh: single-quote every glob passed as a flag value — grep -rn --include='*.ts' pat ., rg -g '*.ts', find . -name '*.md'. An unmatched bare glob aborts the whole command.",
    failing: true,
    per: "shell",
    probes: [[/quot/i, /glob/i], [/--include=['"]/]],
    advice: `make the shell forgiving instead: setopt nonomatch in ${ZSHENV}, or a PreToolUse hook that quotes flag globs`,
  },
  "zsh-glob-path": {
    kind: "rule",
    label: "unmatched path glob (zsh no matches found)",
    fix: "zsh: an unmatched glob aborts the command even with 2>/dev/null. List with find or fd, or quote the pattern.",
    failing: true,
    per: "shell",
    // Not `quote`: a line about quoting flag globs is the flag rule, and says
    // nothing about probing a path that may not exist.
    // `Glob tool` is Claude's file finder, not a shell glob: "use the Glob tool
    // instead of find" says nothing about zsh.
    probes: [
      [
        SHELL_GLOB,
        /\(N\)|nomatch|null_?glob|unmatched|no matches found|path glob|globs? (?:in|for|on) (?:a )?paths?/i,
      ],
      [/\bzsh\b/i, SHELL_GLOB, /\bls -d\b|\bfind\b|\bfd\b/],
    ],
    advice: `setopt nonomatch in ${ZSHENV}, or a PreToolUse hook, so an unmatched glob stays a word`,
  },
  "zsh-equals-abort": {
    kind: "rule",
    label: "unquoted echo === separator",
    fix: 'quote the separator: echo "===" — zsh expands a bare === to a command name, exits 1, and never runs the rest of the line',
    failing: true,
    per: "shell",
    // A bare `===` is JavaScript's equality as often as a separator: it counts
    // only next to echo, a separator or zsh.
    probes: [
      [/\becho\s+["'`]?=/],
      [/===/, /\becho\b|\bseparator|\bzsh\b/i],
      [/\b(?:un)?setopt\s+(?:no_?)?equals\b/i],
    ],
    advice: `unsetopt equals in ${ZSHENV}, so === is a plain word`,
  },
  "pkill-self-kill": {
    kind: "rule",
    label: "pkill -f matched and killed its own shell (exit 144)",
    fix: "pkill -f PAT matches the harness shell, whose command line holds the whole call, and kills it (exit 144). Kill by name (pkill -x NAME) or by a saved PID (kill \"$(cat srv.pid)\"). A bracketed '[p]attern' helps only when no other part of the call repeats the plain text: pkill -f '[b]un run dev'; bun run dev & still dies.",
    failing: true,
    per: "shell",
    probes: [[/pkill/]],
    advice:
      "a PreToolUse hook that rejects pkill -f, pgrep -f | kill and ps | grep | kill in favour of pkill -x NAME or a PID file: a bracketed pattern still kills the shell when the call repeats its plain text",
  },
  "sleep-blocked": {
    kind: "rule",
    label: "harness blocked sleep N followed by a command",
    fix: "Don't sleep N && cmd: wait with an until-loop (until grep -q DONE f; do sleep 2; done), Monitor, or run_in_background.",
    failing: true,
    per: "shell",
    probes: [[/\bsleep\b/, /Monitor|until|background/i]],
    advice:
      "the harness already blocks it: put the wait pattern in the workflow or subagent prompt that keeps hitting it",
  },
  "command-not-found": {
    kind: "env",
    label: "command not found",
    fix: `install the missing binaries, or put them on PATH in ${PATH_FILES}`,
    failing: true,
    per: "shell",
    probes: [],
    advice: `install the binaries on this host, or fix PATH (${PATH_FILES}): a memory line cannot add a binary`,
  },
  "output-too-large": {
    kind: "rule",
    label: "shell output too large: persisted to a file and read back",
    fix: "Bound shell output before it lands: | head -n 200, or write to a file and Read a range.",
    failing: false,
    per: "shell",
    // A rule about bounding any command's output. A `git diff` line covers only
    // a row whose hits are mostly `git diff` (`GIT_DIFF_OUTPUT`).
    probes: [
      [/\|\s*(?:head|tail)\b/, /\boutput\b/i],
      [
        /\b(?:shell|command|bash|tool|terminal) output\b|\boutput of (?:a |the )?(?:shell|command)/i,
        /\bbound|\blimit|\bcap(?:s|ped)?\b|truncat|\bhead\b|\btail\b/i,
      ],
    ],
    advice:
      "a PreToolUse hook that pipes long-output commands (cat, grep -r, logs, a bare git diff) through head or adds --stat to git diff, or name the bound in the workflow prompt",
  },
  "read-before-write": {
    kind: "rule",
    label: "Write/Edit before the file was read",
    fix: "Read a file with the Read tool before Write/Edit: cat or sed in Bash does not count as a read.",
    failing: true,
    per: WRITE_TOOL_NAMES,
    probes: [[/Read tool/i, /\b(?:before|first)\b/i, /\b(?:Edit|Write)\b/]],
    advice:
      "put the Read-first step in the workflow or subagent prompt that writes files",
  },
  "stale-read": {
    kind: "rule",
    label: "Write/Edit after the file changed since it was read",
    fix: "Re-Read a file with the Read tool after a formatter, linter or Bash command touched it, then Write/Edit.",
    failing: true,
    per: WRITE_TOOL_NAMES,
    probes: [[/re-?read/i, /format|lint|modified|changed|touch/i]],
    advice:
      "put the re-Read step in the workflow or subagent prompt that formats then writes",
  },
  "edit-no-match": {
    kind: "rule",
    label: "Edit old_string not found, or not unique",
    fix: "Copy old_string from a fresh Read of the exact lines, and widen it until it is unique.",
    failing: true,
    per: ["Edit", "MultiEdit"],
    // A Read-first line is the read-before-write rule: this one is about what
    // old_string holds.
    probes: [[/old_string/], [/\bEdit\b/, /\bunique\b|exact (?:lines|text)/i]],
    advice:
      "put the fresh-Read step in the workflow or subagent prompt that edits",
  },
  "structured-output-schema": {
    kind: "workflow",
    label: "StructuredOutput did not match its schema",
    fix: "Name every required field in the workflow or subagent prompt that asks for this output.",
    failing: true,
    per: ["StructuredOutput"],
    probes: [],
    advice: null,
  },
  "rm-guard": {
    kind: "rule",
    label: "harness dangerous-rm guard",
    // biome-ignore lint/suspicious/noTemplateCurlyInString: shell syntax, not a template
    fix: 'rm absolute paths, never $VAR/* or a path behind cd: write rm -rf -- "${D:?}"/x so an empty variable cannot widen the target.',
    failing: true,
    per: "shell",
    probes: [[/\brm\b/, /:\?|absolute/]],
    advice: "a PreToolUse hook that rejects rm with a $VAR or cd'd target",
  },
  "scratch-module-resolution": {
    kind: "rule",
    label: "scratch script cannot resolve the project's modules",
    fix: "Run scratch scripts that import project packages from inside the project (bun -e, or a file under the repo), not from /tmp: modules resolve from the script's own directory.",
    failing: true,
    per: "shell",
    // `\bmodules?\b` skips `node_modules`: `_` is a word char, so no boundary.
    probes: [[/scratch|TMPDIR|\/tmp/i, /\bimports?\b|\bmodules?\b|resolv/i]],
    advice:
      "put the scratch-dir rule in the workflow or subagent prompt that writes scripts",
  },
  "ugrep-complexity": {
    kind: "env",
    label: "grep is ugrep: pattern exceeds its complexity limit",
    fix: "grep is aliased to ugrep, which rejects long alternations: use rg or command grep for big patterns.",
    failing: true,
    per: "shell",
    probes: [[/ugrep|command grep/]],
    advice:
      "unalias grep in the shell Claude Code snapshots, or install rg and say so in the prompt",
  },
  unclassified: {
    kind: "none",
    label: "errored, no named cause",
    fix: null,
    failing: true,
    per: "row-tool",
    probes: [],
    advice: null,
  },
};

/** What one detector found on one call. */
export interface CauseHit {
  /** Culprit: glob word, binary, schema field, file basename, `sleep N`. Raw. */
  readonly arg: string | null;
  readonly cause: CauseId;
  /** Failing segments, normalised, `FOCUS_SEP`-joined. Raw. */
  readonly focus: string | null;
  /** The output line that names the cause; the error signature reads it. */
  readonly line: string | null;
}

/** Tools that run a shell, across the four agents. */
export const SHELL_TOOLS: ReadonlySet<string> = new Set([
  "Bash",
  "bash",
  "shell",
  "local_shell",
  "run_command",
  "execute_command",
]);

const GLOB_LINE = /^(?:\(eval\)|zsh):\d+: no matches found: (\S+)/gm;
// The shell may be named by its path: `/bin/bash: line 1: jq: command not found`.
const CNF_LINE =
  /^(?:\(eval\)|(?:\/\S*\/)?(?:zsh|bash))(?::\d+)?:(?: line \d+:)? (?:command not found: (\S+)|(\S+): command not found)/gm;
/** zsh's EQUALS expansion failing: `echo ===` looks up a command named `==`. */
const EQUALS_LINE = /^(?:\(eval\)|zsh):\d+: (=\S*) not found$/gm;
const EXIT_144 = /^Exit code 144\b/;
/**
 * A full-command-line match whose kill can reach the shell running it: `-f`
 * (or `--full`) anywhere among pkill's words, as in `pkill -9 -f x` or
 * `pkill -u $(id -u) -f x`; or `pgrep -f` feeding a kill through xargs, a
 * `$(…)` loop source or a `kill $(pgrep …)`; or `ps | grep PAT | … kill`,
 * whose listing holds the shell's own command line. A bracketed pattern is
 * one too: it still matches the plain text elsewhere in the same call.
 */
const PKILL_SELF =
  /\bpkill\b[^;&|\n]*?\s(?:-\w*f|--full)\b|\bpgrep\b[^;&|\n]*?\s(?:-\w*f|--full)\b[^;\n]*?(?:\|\s*xargs\s+(?:-\S+\s+)*kill\b|\)[^\n]*\bkill\b)|\bkill\s+(?:-\S+\s+)*\$\((?:pgrep|ps)\b|\bps\b[^;&|\n]*\|\s*grep\b[^;\n]*\|[^;\n]*\bkill\b/;
const TOOL_USE_ERROR_OPEN = /^<tool_use_error>/;
const SLEEP_BLOCKED =
  /^Blocked: sleep (\d+) followed by:? ?([\s\S]*?)(?:\. To wait|$)/;
const PERSISTED = /^<persisted-output>\nOutput too large/;
const PERSISTED_PATH = /Full output saved to: (\S+)/;
const NOT_READ = /^File has not been read yet/;
const STALE_READ = /^File has been modified since read/;
const EDIT_NO_MATCH =
  /^(?:String to replace not found|Found \d+ matches of the string)/;
const SCHEMA_FAIL =
  /does not match required schema|could not be parsed as JSON|must NOT have additional properties/;
const SCHEMA_FIELD = /required property '(\w+)'/;
const RM_GUARD =
  /^(?:Dangerous rm operation detected|Permission for this command was denied by a built-in Claude Code safety check[\s\S]{0,200}removals)/;
/**
 * A resolver's module-not-found error, at the start of a line (a log quoting
 * it mid-line, or a grep over transcripts, is not one), capturing the script
 * that imported it: Bun's `Cannot find module 'x' from '<path>'`, Node ESM's
 * (and tsx's) `[ERR_MODULE_NOT_FOUND] ... imported from <path>`, and Node
 * CJS's `Cannot find module 'x'` whose `Require stack:` opens with the script.
 */
const SCRATCH_MODULE: readonly RegExp[] = [
  /^(?:error: )?Cannot find (?:module|package) '[^']+' from '([^']+)'/m,
  /^Error \[ERR_MODULE_NOT_FOUND\]: Cannot find (?:module|package) '[^']+' imported from (?:file:\/\/)?(.+)$/m,
  /^Error: Cannot find module '[^']+'\r?\nRequire stack:\r?\n- (.+)$/m,
];
/**
 * A directory a scratch script lives in: `/tmp`, `/var/tmp`, a `$TMPDIR` such
 * as `~/.cache/agent-tmp` or Windows' `Temp`, macOS's `/var/folders/../T`.
 * Matched per path segment, so `template/` is not one.
 */
const SCRATCH_DIR = /^(?:[^/]*[-_.])?(?:tmp|temp|scratch)(?:[-_.][^/]*|\d*)$/i;
const PATH_SEP = /[\\/]/;
/** Runtimes whose resolver prints one of those errors (npx runs tsx or node). */
const SCRIPT_RUNTIMES = ["bun", "bunx", "node", "tsx", "npx"] as const;
const UGREP = /^ugrep: error: (?:error at position|.*exceeds complexity)/m;
const GREP_WORD = /\b(?:u?grep|egrep)\b/;
const EXIT_HEADER = /^Exit code \d+\n?/;

/** Flags whose value is a glob a program, not the shell, should expand. */
const GLOB_FLAGS: ReadonlySet<string> = new Set([
  "-g",
  "--glob",
  "--include",
  "--exclude",
  "--exclude-dir",
  "--include-dir",
  "-name",
  "-iname",
  "-path",
  "-ipath",
  "-wholename",
]);

const GLOB_CHAR = /[*?[]/;
const WS_RUN = /\s+/;
const WORD_EDGE = "(?:^|[\\s;&|()<>`=])";
const WORD_END = "(?=$|[\\s;&|()<>`])";
const REGEX_SPECIAL = /[.*+?^${}()|[\]\\]/g;

/** Escape a string for use as a literal inside a RegExp. */
export const escapeRe = (s: string): string => s.replace(REGEX_SPECIAL, "\\$&");

/**
 * The first unquoted command word `re` captures in group 1, or null. A quoted
 * word reaches no glob: a grep pattern quoting zsh's message is not one.
 */
const unquotedWord = (cmd: string, re: RegExp): string | null => {
  for (const m of cmd.matchAll(re)) {
    const word = m[1] ?? "";
    if (word && unquotedAt(cmd, m.index + m[0].length - word.length)) {
      return word;
    }
  }
  return null;
};

/** A word that starts with a tilde or a parameter, before its first `/`. */
const EXPANDED_HEAD = String.raw`(?:~[\w.-]*|\$\{?\w+\}?)`;

/**
 * The raw spelling of a culprit the command wrote with backslash escapes:
 * `v2\&p=1` reaches zsh's message as `v2&p=1`. `exact` matches the culprit
 * as a word of the unescaped command.
 */
const escapedSpelling = (cmd: string, exact: RegExp): string | null => {
  if (!cmd.includes("\\")) {
    return null;
  }
  const { text, from } = unescaped(cmd);
  for (const m of text.matchAll(exact)) {
    const word = m[1] ?? "";
    const start = from[m.index + m[0].length - word.length];
    const last = from[m.index + m[0].length - 1];
    if (
      word &&
      start !== undefined &&
      last !== undefined &&
      unquotedAt(cmd, start)
    ) {
      return cmd.slice(start, last + 1);
    }
  }
  return null;
};

/**
 * The command's spelling of zsh's culprit, or null when the culprit is not a
 * word of the command that ran. Brace expansion runs before globbing, so
 * `*.{ts,tsx}` reaches the message as `*.ts`: the text up to the first glob
 * char is accepted instead. Tilde and parameter expansion run before it too,
 * so `ls ~/w/foo*` reaches it as `/home/u/w/foo*`: a command word that starts
 * with `~` or `$VAR` and ends in one of the culprit's `/`-tails is its spelling.
 */
const commandGlob = (cmd: string, word: string): string | null => {
  const exact = new RegExp(`${WORD_EDGE}(${escapeRe(word)})${WORD_END}`, "g");
  if (cmd.includes(word) && unquotedWord(cmd, exact) !== null) {
    return word;
  }
  const escaped = escapedSpelling(cmd, exact);
  if (escaped !== null) {
    return escaped;
  }
  const at = word.search(GLOB_CHAR);
  const head = at > 0 ? word.slice(0, at + 1) : null;
  const braced =
    head !== null && cmd.includes(head)
      ? unquotedWord(
          cmd,
          new RegExp(`${WORD_EDGE}(${escapeRe(head)}[^\\s;&|()<>\`]*)`, "g")
        )
      : null;
  if (braced?.includes("{")) {
    return word;
  }
  for (let slash = word.indexOf("/"); slash >= 0 && slash < at; ) {
    const tail = word.slice(slash);
    const m = cmd.includes(tail)
      ? new RegExp(
          `${WORD_EDGE}(${EXPANDED_HEAD}${escapeRe(tail)})${WORD_END}`
        ).exec(cmd)
      : null;
    if (m?.[1]) {
      return m[1];
    }
    slash = word.indexOf("/", slash + 1);
  }
  return null;
};

/** The token in front of the culprit in the command, for the flag split. */
const precedingToken = (cmd: string, word: string): string => {
  const at = cmd.indexOf(word);
  const head = (at >= 0 ? cmd.slice(0, at) : "").trimEnd();
  return head.split(WS_RUN).at(-1) ?? "";
};

/** A flag glob: the culprit is a flag value, or the value of a glob flag. */
const isFlagGlob = (cmd: string, word: string): boolean => {
  if (word.startsWith("-")) {
    return true;
  }
  return GLOB_FLAGS.has(precedingToken(cmd, word));
};

/** Wrappers that run the next word as the command: the lexer's one list. */
const WRAPPER = String.raw`(?:(?:${[...UNARY_WRAPPERS].join("|")})\s+(?:(?:-\S+|${WRAPPER_OPERAND})\s+)*)*`;
const ENV_PREFIX = String.raw`(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*`;

/**
 * One use of `bin` at a command position: group 1 is the wrapper run in front
 * of it, group 2 the arguments up to the end of its simple command, `2>&1`
 * and `&>` redirects included.
 */
const binUse = (bin: string, flags?: string): RegExp =>
  new RegExp(
    `(?:^|[;&|(\`\\n{]|\\$\\(|\\b(?:then|do|else)\\s)\\s*${ENV_PREFIX}(${WRAPPER})(?:\\S*/)?${escapeRe(bin)}(?=$|[\\s;&|)\`])((?:>&|&>|[^;&|\\n)\`])*)`,
    flags
  );

/** True when `bin` sits at a command position of `cmd`. */
const atCommandPosition = (cmd: string, bin: string): boolean =>
  binUse(bin).test(cmd);

/** Arguments that only ask a binary whether it is there, redirects allowed. */
const VERSION_ARGS =
  /^\s+(?:--version|-v|-V|version)(?:\s+[\d&]*>>?&?\s*\S+)*\s*$/;
const COMMAND_V = /\bcommand\s+(?:-\S+\s+)*-[vV]\b/;

/**
 * True when `cmd` runs `bin` for real at some command position: not only as
 * `bin --version` or through `command -v`. `which`/`type` never put `bin` at a
 * command position. Each use is judged on its own: `which psql; psql -c x`
 * probes and then runs it, and the not-found line comes from the run.
 */
const runsForReal = (cmd: string, bin: string): boolean => {
  for (const m of cmd.matchAll(binUse(bin, "g"))) {
    if (!(COMMAND_V.test(m[1] ?? "") || VERSION_ARGS.test(m[2] ?? ""))) {
      return true;
    }
  }
  return false;
};

const joinFocus = (segs: readonly string[]): string | null =>
  segs.length === 0 ? null : segs.join(FOCUS_SEP);

/**
 * Segments of a command as the retry side will see it: a fact stores its
 * command whitespace-collapsed, so the focus is cut from the same form, and a
 * failed segment and a retry segment are always split the same way.
 */
const focusSegments = (cmd: string): readonly string[] =>
  segments(collapseWs(cmd));

/** Hides secrets in raw text before a focus is cut from it. */
type Scrub = (s: string) => string;

/**
 * Segments that contain the culprit glob, normalised. Cut from the scrubbed
 * command: normalising strips the quotes a secret pattern may be anchored on.
 */
const globFocus = (cmd: string, word: string, scrub: Scrub): string | null => {
  const w = normSeg(word);
  return joinFocus(focusSegments(scrub(cmd)).filter((s) => s.includes(w)));
};

/** True when a segment word runs `bin`, by name or by path. */
const runsBin = (word: string, bin: string): boolean =>
  word === bin || word.endsWith(`/${bin}`);

/**
 * Segments that run the missing binary, with everything up to it dropped: an
 * env prefix or a wrapper (`FOO=1 sudo jq .x f`) is not part of what failed.
 */
const cnfFocus = (cmd: string, bin: string, scrub: Scrub): string | null =>
  joinFocus(
    focusSegments(scrub(cmd)).flatMap((s) => {
      const words = s.split(" ");
      const at = words.findIndex((w) => runsBin(w, bin));
      return at < 0 ? [] : [words.slice(at + 1).join(" ")];
    })
  );

/** The output with Claude's exit header and a harness error tag removed. */
const harnessBody = (output: string): string =>
  output.replace(EXIT_HEADER, "").replace(TOOL_USE_ERROR_OPEN, "");

const hit = (
  cause: CauseId,
  rest: Partial<Omit<CauseHit, "cause">> = {}
): CauseHit => ({
  cause,
  arg: rest.arg ?? null,
  focus: rest.focus ?? null,
  line: rest.line ?? null,
});

/** What every shell detector reads. */
interface ShellInput {
  readonly cmd: string;
  readonly output: string;
  readonly scrub: Scrub;
}

const detectGlob = ({ cmd, output, scrub }: ShellInput): CauseHit | null => {
  if (!output.includes("no matches found")) {
    return null;
  }
  for (const m of output.matchAll(GLOB_LINE)) {
    const culprit = m[1] ?? "";
    const word = culprit === "" ? null : commandGlob(cmd, culprit);
    if (word !== null) {
      return hit(isFlagGlob(cmd, word) ? "zsh-glob-flag" : "zsh-glob-path", {
        arg: word,
        focus: globFocus(cmd, word, scrub),
        line: m[0],
      });
    }
  }
  return null;
};

/**
 * zsh's own `== not found` line, for a word of the command that starts with
 * one more `=`. The command alone is not enough: with `setopt no_equals` (or
 * under bash) `echo ===` is a plain word and nothing failed.
 */
const detectEquals = ({ cmd, output }: ShellInput): CauseHit | null => {
  if (!output.includes(": =")) {
    return null;
  }
  for (const m of output.matchAll(EQUALS_LINE)) {
    const word = `=${m[1] ?? ""}`;
    if (new RegExp(`${WORD_EDGE}${escapeRe(word)}`).test(cmd)) {
      return hit("zsh-equals-abort", { arg: word, line: m[0] });
    }
  }
  return null;
};

const detectCnf = ({ cmd, output, scrub }: ShellInput): CauseHit | null => {
  if (!output.includes("command not found")) {
    return null;
  }
  for (const m of output.matchAll(CNF_LINE)) {
    const bin = m[1] ?? m[2] ?? "";
    if (bin && runsForReal(cmd, bin)) {
      return hit("command-not-found", {
        arg: bin,
        focus: cnfFocus(cmd, bin, scrub),
        line: m[0],
      });
    }
  }
  return null;
};

const detectSleep = (body: string, scrub: Scrub): CauseHit | null => {
  const m = SLEEP_BLOCKED.exec(body);
  if (!m) {
    return null;
  }
  const rest = normSeg(scrub(m[2] ?? ""));
  return hit("sleep-blocked", {
    arg: `sleep ${m[1] ?? ""}`,
    focus: rest === "" ? null : rest,
    line: `Blocked: sleep ${m[1] ?? ""}`,
  });
};

/** True when a script path sits in a scratch directory, not in a project. */
const isScratchPath = (path: string): boolean =>
  path.includes("/var/folders/") ||
  path
    .split(PATH_SEP)
    .slice(0, -1)
    .some((dir) => SCRATCH_DIR.test(dir));

const detectScratch = ({ cmd, output }: ShellInput): CauseHit | null => {
  if (!output.includes("Cannot find ")) {
    return null;
  }
  const m = SCRATCH_MODULE.map((re) => re.exec(output)).find(
    (found) => found !== null
  );
  return m &&
    isScratchPath(m[1] ?? "") &&
    SCRIPT_RUNTIMES.some((bin) => atCommandPosition(cmd, bin))
    ? hit("scratch-module-resolution", { line: m[0].split("\n")[0] ?? m[0] })
    : null;
};

const basename = (path: string): string => path.split(PATH_SEP).at(-1) ?? path;

const detectPersisted = (output: string): CauseHit | null => {
  if (!(output.startsWith("<persisted-output>") && PERSISTED.test(output))) {
    return null;
  }
  const path = PERSISTED_PATH.exec(output)?.[1] ?? null;
  return hit("output-too-large", {
    arg: path === null ? null : basename(path),
  });
};

/**
 * Detectors that read the shell's own messages, in priority order. A
 * persisted output comes last: its preview can still name the real failure,
 * such as a glob abort.
 */
const detectShell = (input: ShellInput): CauseHit | null => {
  const { cmd, output } = input;
  const glob = detectGlob(input);
  if (glob) {
    return glob;
  }
  if (EXIT_144.test(output) && PKILL_SELF.test(cmd)) {
    return hit("pkill-self-kill", { line: "Exit code 144" });
  }
  const body = harnessBody(output);
  if (body.startsWith("Blocked: sleep")) {
    return detectSleep(body, input.scrub);
  }
  const named = detectCnf(input) ?? detectEquals(input);
  if (named) {
    return named;
  }
  if (RM_GUARD.test(body)) {
    return hit("rm-guard", { line: body.split("\n")[0] ?? null });
  }
  const scratch = detectScratch(input);
  if (scratch) {
    return scratch;
  }
  if (output.includes("ugrep: error") && GREP_WORD.test(cmd)) {
    const m = UGREP.exec(output);
    if (m) {
      return hit("ugrep-complexity", { line: m[0] });
    }
  }
  return detectPersisted(output);
};

/** Detectors that read a harness message on a file-editing tool. */
const detectWrite = (tool: string, body: string): CauseHit | null => {
  if (NOT_READ.test(body)) {
    return hit("read-before-write");
  }
  if (STALE_READ.test(body)) {
    return hit("stale-read");
  }
  if ((tool === "Edit" || tool === "MultiEdit") && EDIT_NO_MATCH.test(body)) {
    return hit("edit-no-match");
  }
  return null;
};

const detectSchema = (output: string): CauseHit | null => {
  if (!SCHEMA_FAIL.test(output)) {
    return null;
  }
  return hit("structured-output-schema", {
    arg: SCHEMA_FIELD.exec(output)?.[1] ?? null,
  });
};

const keep: Scrub = (s) => s;

/**
 * The cause of one call, or null. `cmd` is the raw command line for a shell
 * call and empty otherwise. `scrub` redacts the raw text a focus is cut from.
 */
export const detectCause = (args: {
  readonly tool: string;
  readonly cmd: string;
  readonly output: string;
  readonly scrub?: Scrub;
}): CauseHit | null => {
  const { tool, cmd, output, scrub = keep } = args;
  if (SHELL_TOOLS.has(tool)) {
    return detectShell({ cmd, output, scrub });
  }
  if (WRITE_TOOLS.has(tool)) {
    return detectWrite(tool, harnessBody(output));
  }
  if (tool === "StructuredOutput") {
    return detectSchema(output);
  }
  return null;
};

/** The glob a culprit word passes: a flag value loses its `--flag=`. */
export const globValue = (word: string): string => {
  const eq = word.indexOf("=");
  return word.startsWith("-") && eq > 0 ? word.slice(eq + 1) : word;
};
