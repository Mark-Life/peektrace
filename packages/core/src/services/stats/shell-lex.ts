/**
 * Shell lexing: the primitives both halves of the normaliser stand on.
 *
 * A command is split into segments on `&&`, `||`, `;`, a pipe or a newline, and
 * each segment into words — both quote-aware, because a separator inside a
 * quoted span, a `$(...)` or a heredoc body is text, not structure. Getting this
 * wrong is what makes `grep -E "a|b"` read as two commands.
 */

/** One shell word, with whether it arrived quoted or substituted. */
export interface Word {
  quoted: boolean;
  text: string;
}

/** A heredoc introducer and everything up to its closing word. */
const HEREDOC = /<<-?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\1[\s\S]*?^\s*\2\s*$/gm;

/** Replace heredoc bodies with a marker so their contents never reach a family. */
export const stripHeredocs = (cmd: string) => cmd.replace(HEREDOC, "<<HEREDOC");

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
