/** Tokenizer edge snippets for the TUI parity stage: each language, the
 * hand-written scanners (templates, regex literals, heredocs, fences) and
 * text shapes that cross byte and UTF-16 boundaries. */
export interface HighlightCase {
  readonly code: string;
  readonly lang: "bash" | "json" | "markdown" | "text" | "typescript";
  readonly name: string;
}

/** `$` and `{` kept apart, so no string reads as a template slot. */
const O = "${";

const heredocs = "cat <<'EOF'\na << b\nEOF\n".repeat(1000).slice(0, 20_000);

export const HIGHLIGHT_CASES: readonly HighlightCase[] = [
  { name: "ts-basic", lang: "typescript", code: "const x: number = 1 // hi" },
  { name: "ts-block", lang: "typescript", code: "/* a\nb */\nlet y" },
  {
    name: "ts-template",
    lang: "typescript",
    code: `const s = \`a${O}b + \`n${O}c}\`}c ${O} {k: 1}.k }\` + 'q' + "d\\"e"`,
  },
  {
    name: "ts-regex",
    lang: "typescript",
    code: "const r = /[a/b]+\\/x/gi; const d = a / b / c; return /x/.test(s)",
  },
  {
    name: "ts-object",
    lang: "typescript",
    code: "const o = {\n  a: 1, /* c */ b: 'x',\n  // line\n  c$: fn(1.5e3),\n  ...rest,\n};\nfoo({ key: value }); if (x) { y: 1 }",
  },
  {
    name: "ts-types",
    lang: "typescript",
    code: "export const f = async <T,>(a: Array<T>, m: Map<string, Promise<void>>): Promise<T | undefined> => { await a.b.c(); @dec class K extends Base {} return 0x1f + 0b10n + 1_000 }",
  },
  {
    name: "ts-unterminated",
    lang: "typescript",
    code: `const a = 'open\nconst b = \`tpl ${O}x\nconst c = /* never`,
  },
  {
    name: "ts-unicode",
    lang: "typescript",
    code: `const é = '日本語 😀'; const s = \`${O} '🎉' }\`; // nbsp\nfoo bar(1)`,
  },
  {
    name: "ts-crlf",
    lang: "typescript",
    code: "let a = 1;\r\n// c\r\nlet b = 'x'\r\n",
  },
  {
    name: "bash-basic",
    lang: "bash",
    code: 'echo "hi $USER" \'single\' # comment\nexport FOO=1 && ls -la | grep -v x; for f in *.ts; do cat "$f"; done',
  },
  {
    name: "bash-heredoc",
    lang: "bash",
    code: "cat <<EOF\nbody $X\nEOF\ncat <<-'END' > f\n\tline\n\tEND\ngit commit -m \"$(cat <<'MSG'\nmsg\nMSG\n)\"\nnever <<NOPE\nopen",
  },
  {
    name: "bash-vars",
    lang: "bash",
    code: `A=1 B_C=2 cmd ${O}HOME}/x ${O}unterminated\nif [ -f x ]; then echo ok; elif true; then :; fi #end`,
  },
  { name: "bash-heredoc-cap", lang: "bash", code: heredocs },
  {
    name: "bash-crlf",
    lang: "bash",
    code: "echo a\r\ncat <<EOF\r\nx\r\nEOF\r\nls\r\n",
  },
  {
    name: "json-basic",
    lang: "json",
    code: '{\n  "a": 1,\n  "b": [true, false, null, -1.5e10],\n  "c": "str \\"q\\"",\n  // c\n  "d" : {"e":"f"}\n}',
  },
  { name: "json-unterminated", lang: "json", code: '{"a": "open\n, "b": 2}' },
  {
    name: "md-basic",
    lang: "markdown",
    code: "# Title\n\nSome `code` and [link](http://x) ![img](y.png)\n\n- item\n* star\n1. one\n> quote\n\nSetext\n======\nSub\n---",
  },
  {
    name: "md-fences",
    lang: "markdown",
    code: '```ts\nconst a = 1\n```\n\n~~~bash extra\nls -la\n~~~\n\n````md\n```json\n{"x": 1}\n```\n````\n```\nplain\n```\n```unknown\nzz\n```\n```js\nopen fence',
  },
  {
    name: "md-crlf",
    lang: "markdown",
    code: "# Head\r\ntext\r\n```bash\r\nls\r\n```\r\n",
  },
  {
    name: "md-emoji",
    lang: "markdown",
    code: "## 😀 Heading\n`日本` [😀](u)\n",
  },
  { name: "text", lang: "text", code: "const x\n\nline" },
  { name: "empty", lang: "typescript", code: "" },
  {
    name: "nested-quotes",
    lang: "bash",
    code: 'echo "a \'b\' \\" c" \'d "e"\' x',
  },
];
