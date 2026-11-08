/** `.claude/rules/*.md` files Claude Code loads at session start. */
import { posix } from "node:path";
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";

const FRONTMATTER = /^﻿?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/;
const PATHS_KEY = /^paths\s*:(.*)$/;
const LIST_ITEM = /^\s+-\s*(.*)$/;
const NEWLINE = /\r?\n/;
const BRACKETS = /^\[|\]$/g;
const QUOTED = /^(["'])(.*)\1$/;
const MAX_DEPTH = 8;
const ANY_DEPTH = "/**";

/** Split on commas outside `{…}`, as Claude Code splits a `paths` value. */
const splitGlobs = (value: string) => {
  const out: string[] = [];
  let depth = 0;
  let part = "";
  for (const ch of value) {
    if (ch === "," && depth === 0) {
      out.push(part);
      part = "";
    } else {
      depth += ch === "{" ? 1 : 0;
      depth -= ch === "}" ? 1 : 0;
      part += ch;
    }
  }
  return [...out, part];
};

/** The `paths` globs in a rule's frontmatter; empty when it has none. */
export const ruleGlobs = (text: string) => {
  const block = FRONTMATTER.exec(text)?.[1];
  const lines = block?.split(NEWLINE) ?? [];
  const at = lines.findIndex((l) => PATHS_KEY.test(l));
  if (at < 0) {
    return [];
  }
  const inline = PATHS_KEY.exec(lines[at] ?? "")?.[1]?.trim() ?? "";
  const items: string[] = [];
  for (const l of lines.slice(at + 1)) {
    const item = LIST_ITEM.exec(l)?.[1];
    if (item === undefined) {
      break;
    }
    items.push(item);
  }
  const raw = inline === "" ? items : [inline.replace(BRACKETS, "")];
  return raw
    .flatMap(splitGlobs)
    .map((g) => g.trim().replace(QUOTED, "$2").trim())
    .map((g) => (g.endsWith(ANY_DEPTH) ? g.slice(0, -ANY_DEPTH.length) : g))
    .filter((g) => g.length > 0);
};

/** A rule with `paths` globs loads only when a matching file is touched. */
export const isConditionalRule = (text: string) => {
  const globs = ruleGlobs(text);
  return globs.length > 0 && !globs.every((g) => g === "**");
};

/** An unconditional `.md` rule file at `path`. */
const isLoadedRule = (fs: FileSystem.FileSystem, path: string) =>
  path.endsWith(".md")
    ? fs.readFileString(path).pipe(
        Effect.map((text) => !isConditionalRule(text)),
        Effect.orElseSucceed(() => false)
      )
    : Effect.succeed(false);

/**
 * Unconditional `.md` rules under `dir`, subdirs included, in name order.
 * Empty when `dir` is missing.
 */
export const rulePaths = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly dir: string;
}): Effect.Effect<string[]> => {
  const { fs } = args;
  const seen = new Set<string>();
  const walk = (dir: string, depth: number): Effect.Effect<string[]> =>
    Effect.gen(function* () {
      const real = yield* fs
        .realPath(dir)
        .pipe(Effect.orElseSucceed(() => dir));
      if (seen.has(real) || depth > MAX_DEPTH) {
        return [];
      }
      seen.add(real);
      const names = yield* fs
        .readDirectory(dir)
        .pipe(Effect.orElseSucceed(() => [] as string[]));
      const out: string[] = [];
      for (const name of [...names].sort()) {
        const path = posix.join(dir, name);
        const info = yield* fs
          .stat(path)
          .pipe(Effect.orElseSucceed(() => null));
        if (info?.type === "Directory") {
          out.push(...(yield* walk(path, depth + 1)));
        } else if (info?.type === "File" && (yield* isLoadedRule(fs, path))) {
          out.push(path);
        }
      }
      return out;
    });
  return walk(args.dir, 0);
};
