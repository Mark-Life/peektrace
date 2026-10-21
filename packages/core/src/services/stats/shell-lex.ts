/**
 * Shell lexing: the primitives the normaliser and the detectors stand on.
 *
 * A command is split into segments on `&&`, `||`, `;`, a pipe or a newline, and
 * each segment into words — both quote-aware, because a separator inside a
 * quoted span, a `$(...)` or a heredoc body is text, not structure. Getting this
 * wrong is what makes `grep -E "a|b"` read as two commands.
 *
 * Heredocs, and the per-char context a detector asks about (is this `*` code
 * or quoted?), come from one state machine at the end of this file. The
 * segment and word scans keep their own lighter quote flags: they also track
 * backticks, which that machine does not.
 */

/** One shell word, with whether it arrived quoted or substituted. */
export interface Word {
  quoted: boolean;
  text: string;
}

/** Quote and nesting flags carried through one scan. */
interface ScanState {
  backtick: boolean;
  depth: number;
  double: boolean;
  single: boolean;
}

const freshState = (): ScanState => ({
  single: false,
  double: false,
  backtick: false,
  depth: 0,
});

/** What one character does to a segment scan. */
type SplitAction = "background" | "cut" | "cut-two" | "escape" | "keep";

const WS = /\s/;

/**
 * Update the quote and nesting flags for one char. True when the char belongs
 * to a quoted or nested span, so no separator inside it can cut a segment.
 */
const isQuoted = (st: ScanState, c: string): boolean => {
  if (c === "'" && !(st.double || st.backtick)) {
    st.single = !st.single;
    return true;
  }
  if (c === '"' && !st.single) {
    st.double = !st.double;
    return true;
  }
  if (st.single || st.double) {
    return true;
  }
  if (c === "`") {
    st.backtick = !st.backtick;
    return true;
  }
  if (c === "(") {
    st.depth++;
    return true;
  }
  if (c === ")") {
    st.depth = Math.max(0, st.depth - 1);
    return true;
  }
  return st.depth > 0 || st.backtick;
};

/** Classify one character, updating the quote and nesting flags in place. */
const splitAction = (args: {
  readonly c: string;
  readonly next: string | undefined;
  readonly prev: string | undefined;
  readonly st: ScanState;
}): SplitAction => {
  const { st, c, next, prev } = args;
  if (c === "\\" && !st.single) {
    return "escape";
  }
  if (isQuoted(st, c)) {
    return "keep";
  }
  if ((c === "&" && next === "&") || (c === "|" && next === "|")) {
    return "cut-two";
  }
  if (c === "|" || c === ";" || c === "\n") {
    return "cut";
  }
  if (c === "&" && prev !== ">" && next !== ">") {
    return "background";
  }
  return "keep";
};

/**
 * Split a command into top-level segments on `&&`, `||`, `;`, a pipe or a
 * newline, ignoring separators inside quotes, `$(...)`, backticks or `(...)`.
 * Also reports whether the command ends in a background `&`.
 */
export const splitSegments = (cmd: string) => {
  const st = freshState();
  const segments: string[] = [];
  let buf = "";
  let background = false;
  const push = () => {
    if (buf.trim()) {
      segments.push(buf.trim());
    }
    buf = "";
  };
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i] ?? "";
    const next = cmd[i + 1];
    const action = splitAction({ st, c, next, prev: cmd[i - 1] });
    if (action === "escape") {
      buf += c + (next ?? "");
      i++;
    } else if (action === "cut-two") {
      push();
      i++;
    } else if (action === "cut") {
      push();
    } else if (action === "background") {
      background = true;
      push();
    } else {
      buf += c;
    }
  }
  push();
  return { segments, background };
};

/** What one character does to a word scan. */
type TokenAction = "escape" | "keep" | "quote" | "space";

const tokenAction = (args: {
  readonly c: string;
  readonly next: string | undefined;
  readonly st: ScanState;
}): TokenAction => {
  const { st, c, next } = args;
  if (c === "\\") {
    return "escape";
  }
  if (c === "'" && !st.double) {
    st.single = !st.single;
    return "quote";
  }
  if (c === '"' && !st.single) {
    st.double = !st.double;
    return "quote";
  }
  if (st.single || st.double) {
    return "keep";
  }
  if (c === "(" || (c === "$" && next === "(")) {
    st.depth++;
    return "keep";
  }
  if (c === ")") {
    st.depth = Math.max(0, st.depth - 1);
    return "keep";
  }
  return st.depth === 0 && WS.test(c) ? "space" : "keep";
};

/** Split one segment into quote-aware words, keeping `$(...)` as a single word. */
export const tokenize = (segment: string) => {
  const st = freshState();
  const words: Word[] = [];
  let buf = "";
  let quoted = false;
  const push = () => {
    if (buf) {
      words.push({ text: buf, quoted });
    }
    buf = "";
    quoted = false;
  };
  for (let i = 0; i < segment.length; i++) {
    const c = segment[i] ?? "";
    const action = tokenAction({ st, c, next: segment[i + 1] });
    if (action === "escape") {
      buf += segment[i + 1] ?? "";
      i++;
    } else if (action === "quote") {
      quoted = true;
    } else if (action === "space") {
      push();
    } else {
      buf += c;
    }
  }
  push();
  return words;
};

const FLAG = /^-{1,2}[^-\s]/;

/** True when the word is a flag such as `-v`, `--filter`, or `--filter=web`. */
export const isFlag = (w: string) => FLAG.test(w) || w === "--";

/** Leading redirection operator, e.g. `>`, `>>`, `2>`, `&>`, `<`, `2>&1`. */
export const REDIRECT = /^(?:\d?(?:>>|>|<<<|<<|<)|&>>?)/;

const ENV_ASSIGN = /^[A-Za-z_][A-Za-z0-9_]*=/;

/** True when the word is `FOO=bar`, an env assignment in front of the command. */
export const isEnvAssign = (w: string) => ENV_ASSIGN.test(w);

const EXTENSION = /\.[A-Za-z0-9]{1,6}$/;
const NUMBER_WORD = /^\d+(?:[.,:]\d+)*$/;
const HEX_WORD = /^[0-9a-f]{7,}$/i;
const UUID_WORD =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const URL_WORD = /^[a-z][a-z0-9+.-]*:\/\//i;
const SLASH = /[\\/]/;
const EXE = /\.exe$/i;

/** True when the word names a file or directory rather than a keyword. */
export const isPathLike = (w: string) =>
  w.includes("/") ||
  w.includes("\\") ||
  w.startsWith("~") ||
  w.startsWith(".") ||
  EXTENSION.test(w);

/** True when the word is a number, a hash, a UUID, a variable or a URL. */
export const isLiteral = (w: string) =>
  NUMBER_WORD.test(w) ||
  HEX_WORD.test(w) ||
  UUID_WORD.test(w) ||
  URL_WORD.test(w) ||
  w.startsWith("$") ||
  w.startsWith("<<");

/** Basename of a path with its extension dropped: the script's own name. */
export const scriptBase = (text: string) =>
  (text.split(SLASH).pop() ?? text).replace(EXTENSION, "");

/** Strip a directory prefix and a `.exe` suffix from a binary name. */
export const binName = (w: string) => {
  const base = w.split(SLASH).pop() ?? w;
  return base.replace(EXE, "");
};

/**
 * Wrappers that run their remaining words as the command, past their own
 * flags and operands. The one list the normaliser, the cause detectors and
 * the retry pairer all unwrap with.
 */
export const UNARY_WRAPPERS: ReadonlySet<string> = new Set([
  "bunx",
  "command",
  "env",
  "exec",
  "nice",
  "nohup",
  "npx",
  "pnpx",
  "stdbuf",
  "sudo",
  "time",
  "timeout",
  "xargs",
]);

/** Two-word wrappers: `<bin> <word>` runs the next word as the real command. */
const BINARY_WRAPPERS: Readonly<Record<string, ReadonlySet<string>>> = {
  bun: new Set(["x"]),
  npm: new Set(["exec", "x"]),
  pnpm: new Set(["dlx", "exec"]),
  yarn: new Set(["dlx"]),
};

/**
 * A word a wrapper takes before the command: a flag's value or timeout's
 * duration, as in `nice -n 10`, `timeout 30s`, `xargs -I {}`.
 */
export const WRAPPER_OPERAND = String.raw`(?:\d+(?:\.\d+)?[smhd]?|\{\})`;
const OPERAND_WORD = new RegExp(`^${WRAPPER_OPERAND}$`);

/** Wrapper hops unwrapped before giving up on a nested command. */
const UNWRAP_GUARD = 4;

/**
 * Index of the real command among a segment's words: past leading env
 * assignments, then past each wrapper with its flags, operands and env
 * assignments. `words.length` when there is none.
 */
export const commandIndex = (words: readonly string[]): number => {
  const at = (i: number) => words[i] ?? "";
  let i = 0;
  while (i < words.length && isEnvAssign(at(i))) {
    i++;
  }
  for (let guard = 0; guard < UNWRAP_GUARD && i < words.length; guard++) {
    const head = binName(at(i));
    if (UNARY_WRAPPERS.has(head)) {
      i++;
      while (
        i < words.length &&
        (isEnvAssign(at(i)) || isFlag(at(i)) || OPERAND_WORD.test(at(i)))
      ) {
        i++;
      }
      continue;
    }
    if (BINARY_WRAPPERS[head]?.has(binName(at(i + 1)))) {
      i += 2;
      continue;
    }
    break;
  }
  return i;
};

/**
 * How the shell reads each char of a command line: in code, inside quotes,
 * or in a heredoc body.
 *
 * Covered: `'` and `"` quotes, a backslash escaping the next char outside
 * single quotes, `$(...)` opening fresh code (also inside double quotes), and
 * heredoc bodies (`<<EOF`, `<<-EOF`, `<<'EOF'`) up to their delimiter line.
 * Not covered: backticks, `$'...'`, comments; they are rare in agent commands.
 */

/**
 * What a char is to the shell, read before the char itself is consumed:
 * - `code`: plain code; globs expand here.
 * - `subst`: code inside `$(...)` that sits inside double quotes; globs expand.
 * - `quoted`: inside a quote, or escaped by a backslash; literal.
 * - `heredoc`: a heredoc body or its delimiter line; literal, not code.
 */
export type ShellCtx = "code" | "subst" | "quoted" | "heredoc";

type Frame = "'" | '"' | "$(" | "(";

const HEREDOC_OP = /<<(-?)[ \t]*(?:'([^'\n]*)'|"([^"\n]*)"|\\?([\w.-]+))/y;
const LEADING_TABS = /^\t+/;

/** A heredoc whose body starts after the current line. */
interface Pending {
  readonly delim: string;
  readonly stripTabs: boolean;
}

/** The end of the heredoc body that starts at `from`, past its delimiter line. */
const heredocEnd = (cmd: string, from: number, doc: Pending): number => {
  let at = from;
  while (at < cmd.length) {
    const nl = cmd.indexOf("\n", at);
    const end = nl < 0 ? cmd.length : nl;
    const line = cmd.slice(at, end);
    const next = nl < 0 ? cmd.length : nl + 1;
    if ((doc.stripTabs ? line.replace(LEADING_TABS, "") : line) === doc.delim) {
      return next;
    }
    at = next;
  }
  return cmd.length;
};

/** True when the stack is in code: empty, or topped by a substitution. */
const inCode = (stack: readonly Frame[]): boolean => {
  const top = stack.at(-1);
  return top === undefined || top === "$(" || top === "(";
};

/** The context of a code char: `subst` when any quote encloses it. */
const codeCtx = (stack: readonly Frame[]): ShellCtx =>
  stack.includes('"') ? "subst" : "code";

/** Moves the frame stack past one unescaped char. */
const stepFrames = (stack: Frame[], c: string, prev: string): void => {
  const top = stack.at(-1);
  if (top === "'") {
    if (c === "'") {
      stack.pop();
    }
    return;
  }
  if (top === '"') {
    if (c === '"') {
      stack.pop();
    } else if (c === "(" && prev === "$") {
      stack.push("$(");
    }
    return;
  }
  if (c === "'" || c === '"') {
    stack.push(c);
  } else if (c === "(") {
    stack.push(prev === "$" ? "$(" : "(");
  } else if (c === ")" && top !== undefined) {
    stack.pop();
  }
};

/** The lexer's state across one command. */
interface Lexer {
  readonly cmd: string;
  escaped: boolean;
  /** `[start, end)` of every `<<DELIM` operator that opened a heredoc. */
  readonly ops: [number, number][];
  readonly out: ShellCtx[];
  readonly pending: Pending[];
  readonly stack: Frame[];
}

/**
 * At a code `<<` that opens a heredoc: note its delimiter, mark the operator
 * like the `<<`, and return the index past it. Null when it opens none.
 */
const heredocOp = (lx: Lexer, i: number): number | null => {
  const { cmd } = lx;
  if (cmd[i + 1] !== "<" || cmd[i + 2] === "<") {
    return null;
  }
  HEREDOC_OP.lastIndex = i;
  const m = HEREDOC_OP.exec(cmd);
  if (!m) {
    return null;
  }
  lx.pending.push({
    delim: m[2] ?? m[3] ?? m[4] ?? "",
    stripTabs: m[1] === "-",
  });
  lx.out.fill(lx.out[i] ?? "code", i + 1, i + m[0].length);
  lx.ops.push([i, i + m[0].length]);
  return i + m[0].length;
};

/** At a code newline: skip every pending heredoc body, return the index after. */
const heredocBodies = (lx: Lexer, i: number): number => {
  let at = i + 1;
  for (const doc of lx.pending.splice(0)) {
    const end = heredocEnd(lx.cmd, at, doc);
    lx.out.fill("heredoc", at, end);
    at = end;
  }
  return at;
};

/** Consume the char at `i`; return the index of the next one to read. */
const lexChar = (lx: Lexer, i: number): number => {
  const c = lx.cmd[i] ?? "";
  if (lx.escaped) {
    lx.out[i] = "quoted";
    lx.escaped = false;
    return i + 1;
  }
  const code = inCode(lx.stack);
  lx.out[i] = code ? codeCtx(lx.stack) : "quoted";
  if (c === "\\" && lx.stack.at(-1) !== "'") {
    lx.escaped = true;
    return i + 1;
  }
  if (code && c === "<") {
    const past = heredocOp(lx, i);
    if (past !== null) {
      return past;
    }
  }
  if (code && c === "\n" && lx.pending.length > 0) {
    return heredocBodies(lx, i);
  }
  stepFrames(lx.stack, c, lx.cmd[i - 1] ?? "");
  return i + 1;
};

const lex = (cmd: string): Lexer => {
  const lx: Lexer = {
    cmd,
    escaped: false,
    ops: [],
    out: new Array<ShellCtx>(cmd.length),
    pending: [],
    stack: [],
  };
  for (let i = 0; i < cmd.length; ) {
    i = lexChar(lx, i);
  }
  return lx;
};

/** The context of every char of `cmd`, index for index. */
export const shellContexts = (cmd: string): readonly ShellCtx[] => lex(cmd).out;

/**
 * Replace each heredoc with a marker so its body never reaches a family: the
 * `<<DELIM` operator becomes `<<HEREDOC`, and the body and its delimiter line
 * are dropped. A `<<` inside quotes opens nothing.
 */
export const stripHeredocs = (cmd: string): string => {
  const { ops, out } = lex(cmd);
  let text = "";
  let op = 0;
  for (let i = 0; i < cmd.length; i++) {
    const [start, end] = ops[op] ?? [-1, -1];
    if (i === start) {
      text += "<<HEREDOC";
      i = end - 1;
      op++;
    } else if (out[i] !== "heredoc") {
      text += cmd[i] ?? "";
    }
  }
  return text;
};

/** True when the shell reads char `at` of `cmd` as code, where globs expand. */
export const unquotedAt = (cmd: string, at: number): boolean => {
  const ctx = shellContexts(cmd.slice(0, at + 1))[at];
  return ctx === "code" || ctx === "subst";
};

/**
 * `cmd` with each shell backslash escape removed, and for every char of the
 * result the index it came from. zsh prints a culprit as the shell read it:
 * `v2\&p=1` reaches the message as `v2&p=1`.
 */
export const unescaped = (
  cmd: string
): { readonly text: string; readonly from: readonly number[] } => {
  const ctx = shellContexts(cmd);
  let text = "";
  const from: number[] = [];
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i] ?? "";
    const isEscape =
      c === "\\" &&
      ctx[i] !== "heredoc" &&
      ctx[i + 1] === "quoted" &&
      (ctx[i] === "code" || ctx[i] === "subst");
    if (!isEscape) {
      text += c;
      from.push(i);
    }
  }
  return { text, from };
};
