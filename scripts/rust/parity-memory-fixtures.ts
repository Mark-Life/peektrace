/** Memory edge trees for the parity harness: every frontmatter shape, index
 * line style and vault state, plus the seed vault of `memory.test.ts`. */
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { REPO } from "./parity-lib";

const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const INDEX_LINES = 205;
const LATE_LINE = 203;

const put = (path: string, body: string | Uint8Array) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
  utimesSync(path, FIXED_MTIME, FIXED_MTIME);
};

/** `---\nname: \xff\xfe\n---\nbody\n`: invalid UTF-8 in a name. */
const BAD_UTF8 = new Uint8Array([
  0x2d, 0x2d, 0x2d, 0x0a, 0x6e, 0x61, 0x6d, 0x65, 0x3a, 0x20, 0xff, 0xfe, 0x0a,
  0x2d, 0x2d, 0x2d, 0x0a, 0x62, 0x6f, 0x64, 0x79, 0x0a,
]);

const projA = (a: string) => {
  put(
    join(a, "nested.md"),
    '---\nname: nested\ndescription: A note with nested metadata\nmetadata:  \n  node_type: insight\n  type: user\n  name: inner-name\n  originSessionId: "abc123"\n  modified: 2026-01-01\n---\n# Nested\n\nLinks [[flat]] and [[Flat#section|alias]] and [[missing-note]].\n'
  );
  put(
    join(a, "flat.md"),
    "---\ntype: feedback\nname: flat\ndescription: \"A quoted, legacy: description\"\n1: numeric key\nzeta: z\n__proto__: proto\nalpha: 'a'\ntype: project\n---\nSee [b](sub/dir/Nested.md#x), [ext](https://x.dev/a.md), [anchor](#top), ![img](pic.png), [mail](mailto:a@b).\n"
  );
  put(
    join(a, "none.md"),
    "No frontmatter here, links to [[nested]].\r\nSecond line\r\n"
  );
  put(join(a, "unterminated.md"), "---\nname: broken\nno closing fence\n");
  put(
    join(a, "crlf.md"),
    "---\r\nname: crlf\r\ndescription: Windows\r\ntype: reference\r\n---\r\nBody [[nested]]\r\n"
  );
  put(join(a, "bom.md"), "﻿---\nname: bom\ntype: user\n---\nBOM body\n");
  put(join(a, "badutf8.md"), BAD_UTF8);
  // An integer-like type: JS puts it first among the type counts.
  put(join(a, "weird-type.md"), "---\nname: weird\ntype: 42\n---\nx\n");
  put(
    join(a, "self.md"),
    "---\nname: self\ntype: user\n---\n[[self]] [[nestd]] [[ne]]\n"
  );
  put(join(a, "Upper.md"), "---\ntype: user\n---\nUpper body [[upper]]\n");
  put(join(a, "notes.txt"), "stray\n");
  put(join(a, "zz.json"), "{}\n");
  mkdirSync(join(a, "subdir.md"), { recursive: true });
  put(join(a, "locked.md"), "---\nname: locked\n---\nsecret\n");
  chmodSync(join(a, "locked.md"), NO_ACCESS);
  symlinkSync(join(a, "flat.md"), join(a, "link.md"));
  put(
    join(a, "MEMORY.md"),
    [
      "# Index",
      "",
      "- [Nested](nested.md) — nested one",
      "- [Flat](flat.md) – en dash hook",
      "* [None](none.md) · middle dot",
      "- [Crlf](crlf.md) -- double dash",
      "- [Bom](./bom.md#frag)",
      "- [[self]] - wiki hook",
      "- [[Upper|U]]",
      "- [Ghost](ghost.md) — missing file",
      "- [[nested-ghost]]",
      "- bare bullet with no link",
      "- [Ext](https://example.com/x.md) — external",
      "- [Empty hook](weird-type.md) —   ",
      "- [Sep](self.md) — hook more",
      "prose line",
      "",
    ].join("\n")
  );
};

/** Build the memory edge HOME at `out`; returns the files made unreadable. */
export const buildMemHome = (out: string) => {
  rmSync(out, { recursive: true, force: true });
  const projects = join(out, ".claude", "projects");
  const a = join(projects, "-Users-demo-proj-a", "memory");
  projA(a);
  put(
    join(projects, "-Users-demo-proj-b", "memory", "MEMORY.md"),
    "# Notes\n\nSome prose.\n\n## More\n- bullet only\n"
  );
  const c = join(projects, "-Users-demo-proj-c", "memory");
  const lines = Array.from({ length: INDEX_LINES }, (_, i) =>
    i === LATE_LINE ? "- [Late](late.md) — below fold" : `line ${i}`
  );
  put(join(c, "MEMORY.md"), `${lines.join("\n")}\n`);
  put(
    join(c, "late.md"),
    "---\nname: late\ndescription: d\ntype: project\n---\nbody\n"
  );
  put(join(projects, "-Users-demo-only-index", "memory", "MEMORY.md"), "");
  mkdirSync(join(projects, "-Users-demo-empty", "memory"), {
    recursive: true,
  });
  mkdirSync(join(projects, "-Users-demo-nomem"), { recursive: true });
  put(join(projects, "-Users-demo-stray", "memory", "x.txt"), "x");
  put(
    join(projects, "C#proj", "memory", "hash.md"),
    "---\nname: hash\n---\nx\n"
  );
  put(join(projects, "C#proj", "memory", "MEMORY.md"), "- [Hash](hash.md)\n");
  put(join(projects, "plainfile"), "not a dir");
  seedVault(join(projects, "-Users-demo-seed", "memory"));
  put(join(out, "outside", "memory", "x.md"), "---\nname: x\n---\nx\n");
  const work = join(out, "work", "projects", "-work-proj", "memory");
  put(join(work, "w.md"), "---\nname: w\ntype: user\n---\nwork\n");
  put(
    join(out, ".peektrace", "settings.json"),
    JSON.stringify({
      roots: { claude: [{ path: join(out, "work"), label: "work" }] },
    })
  );
  return [join(a, "locked.md")];
};

/** The vault of `memory.test.ts` plus both `fixtures/memory` files. */
const seedVault = (dir: string) => {
  put(
    join(dir, "a.md"),
    "---\nname: a\ndescription: First note\ntype: user\n---\nLinks to [[b]] here.\n"
  );
  put(
    join(dir, "b.md"),
    "---\nname: b\ndescription: Second note\ntype: project\n---\nNo links here.\n"
  );
  put(
    join(dir, "orphan.md"),
    "---\nname: orphan\ndescription: Unindexed note\ntype: reference\n---\nAlone.\n"
  );
  put(
    join(dir, "MEMORY.md"),
    "- [A](a.md) — first\n- [B](b.md) — second\n- [Ghost](ghost.md) — missing\n"
  );
  const fixtures = join(REPO, "packages/core/test/fixtures/memory");
  for (const name of ["nested.md", "flat.md"]) {
    put(join(dir, `fx-${name}`), readFileSync(join(fixtures, name)));
  }
};

/** Slugs that are not projects with memory, for `memory ls <slug>`. */
export const EXTRA_SLUGS = [
  "-Users-demo-empty",
  "-Users-demo-stray",
  "-Users-demo-nomem",
  "plainfile",
  "nosuchproj",
  "../..",
  "",
];

/** Names that are not entries, for `memory show`. */
export const EXTRA_NAMES = ["upper", "locked", "ghost"];
