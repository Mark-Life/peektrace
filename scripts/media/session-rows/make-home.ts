// Builds a synthetic HOME for session-row screenshots: Claude, Codex and Pi
// transcripts, a t3code state DB and real git worktrees of fake repos.
//   bun scripts/media/session-rows/make-home.ts <outdir> [<alias>]
// <outdir> must sit under the repo's .scratch dir; it is wiped first.
// <alias> (/tmp/<name>) becomes a symlink to <outdir>, and every path written
// into transcripts, the DB and git uses it, so captures show a neutral path
// instead of this machine's.
import { execFileSync } from "node:child_process";
import {
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

import { writeT3Db } from "./t3code-db";
import { claude, codex, pi } from "./transcripts";
import type { TranscriptFile } from "./transcripts";

const USAGE_EXIT = 2;
const repo = path.resolve(import.meta.dir, "../../..");
const [outArg, alias] = process.argv.slice(2);

const refuse = (why: string) => {
  console.error(why);
  process.exit(USAGE_EXIT);
};

if (outArg === undefined) {
  refuse(
    "usage: bun scripts/media/session-rows/make-home.ts <outdir> [<alias>]"
  );
}

const home = path.resolve(outArg ?? "");

if (!home.startsWith(`${repo}/.scratch/`)) {
  refuse(`refusing ${home}: it must sit under ${repo}/.scratch`);
}

if (alias !== undefined && !/^\/tmp\/[\w.-]+$/u.test(alias)) {
  refuse(`refusing alias ${alias}: use /tmp/<name>`);
}

const existing =
  alias === undefined ? undefined : lstatSync(alias, { throwIfNoEntry: false });

if (existing !== undefined && !existing.isSymbolicLink()) {
  refuse(`refusing alias ${alias}: it exists and is not a symlink`);
}

rmSync(home, { force: true, recursive: true });

mkdirSync(home, { recursive: true });

const realHome = realpathSync(home);

if (alias !== undefined) {
  rmSync(alias, { force: true });
  symlinkSync(home, alias);
}

// The HOME as transcripts, the DB and git see it.
const base = alias ?? home;

const write = (file: string, text: string) => {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, text);
};

// Git with no user config, a fixed identity and fixed dates.
const gitEnv = {
  GIT_AUTHOR_DATE: "2026-11-10T09:00:00Z",
  GIT_AUTHOR_EMAIL: "demo@example.test",
  GIT_AUTHOR_NAME: "Demo",
  GIT_COMMITTER_DATE: "2026-11-10T09:00:00Z",
  GIT_COMMITTER_EMAIL: "demo@example.test",
  GIT_COMMITTER_NAME: "Demo",
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_CONFIG_NOSYSTEM: "1",
  HOME: base,
  PATH: "/usr/bin:/bin:/opt/homebrew/bin",
};

const git = (cwd: string, ...args: string[]) =>
  execFileSync("git", args, { cwd, env: gitEnv, stdio: "pipe" });

const makeRepo = (dir: string, branch: string) => {
  mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "-b", branch);
  write(path.join(dir, "README.md"), `# ${path.basename(dir)}\n`);
  git(dir, "add", ".");
  git(dir, "commit", "-q", "-m", "init");
};

const addWorktree = (main: string, dir: string, branch: string) => {
  mkdirSync(path.dirname(dir), { recursive: true });
  git(main, "worktree", "add", "-q", "-b", branch, dir);

  // git stores real paths; point both links at the alias paths instead.
  const links = [
    path.join(dir, ".git"),
    path.join(main, ".git/worktrees", path.basename(dir), "gitdir"),
  ];

  for (const link of links) {
    writeFileSync(link, readFileSync(link, "utf-8").replaceAll(realHome, base));
  }
};

const shop = path.join(base, "code/acme-shop");
const api = path.join(base, "code/acme-api");
const docs = path.join(base, "code/acme-docs");

const longProject = path.join(
  base,
  "code/acme-enterprise-customer-billing-and-subscription-management-platform-monorepo-v2"
);

const longBranch =
  "feature/billing-2207-migrate-legacy-invoice-renderer-to-the-new-template-engine-with-pdf-a";

const childWt = path.join(
  base,
  ".t3/worktrees/acme-shop/t3-5f1c9a2e-0b7d4e8a91c3f6d2"
);

const childBranch = "t3/coupon-edge-cases";
const otherWt = path.join(base, ".othertool/worktrees/acme-api/workspace-1");

for (const dir of [shop, api, docs, longProject]) {
  makeRepo(dir, "main");
}

git(longProject, "checkout", "-q", "-b", longBranch);

addWorktree(shop, childWt, childBranch);

addWorktree(api, otherWt, "fix/retry-backoff");

const ids = {
  "claude-long": "e5e5e5e5-0000-4000-8000-000000000005",
  "claude-plain": "f1f1f1f1-0000-4000-8000-000000000006",
  "claude-plain-2": "f2f2f2f2-0000-4000-8000-000000000007",
  codex: "019f6a10-0000-7000-8000-000000000004",
  "pi-nvidia": "019f6a20-0000-7000-8000-000000000003",
  "pi-othertool-worktree": "019f6a21-0000-7000-8000-000000000008",
  "t3code-child-worktree": "b2b2b2b2-0000-4000-8000-000000000002",
  "t3code-parent": "a1a1a1a1-0000-4000-8000-000000000001",
};

const files: readonly TranscriptFile[] = [
  claude({
    branch: "main",
    cwd: shop,
    id: ids["t3code-parent"],
    start: "2026-11-16T08:00:00Z",
    title: "Checkout fixes",
    turns: 110,
  }),
  // t3code starts its worktrees detached, so the transcript sees `HEAD`.
  claude({
    branch: "HEAD",
    cwd: childWt,
    id: ids["t3code-child-worktree"],
    start: "2026-11-16T10:00:00Z",
    title: "Coupon edge cases",
    turns: 120,
  }),
  claude({
    branch: longBranch,
    cwd: longProject,
    id: ids["claude-long"],
    start: "2026-11-15T13:00:00Z",
    tail: "Ship it.",
    title:
      "Migrate the legacy invoice renderer to the new template engine, keep PDF/A output byte-identical for archived invoices, and add regression tests for every currency and locale we bill in today",
    turns: 115,
  }),
  claude({
    branch: "main",
    cwd: docs,
    id: ids["claude-plain"],
    model: "claude-sonnet-4-5",
    start: "2026-11-15T09:00:00Z",
    title: "Update the API docs",
    turns: 24,
  }),
  claude({
    branch: "feature/search",
    cwd: shop,
    id: ids["claude-plain-2"],
    start: "2026-11-14T15:00:00Z",
    title: "Add product search",
    turns: 40,
  }),
  codex({
    branch: "feature/rate-limits",
    cwd: api,
    id: ids.codex,
    model: "gpt-5.5",
    start: "2026-11-16T07:00:00.000Z",
    turns: 30,
  }),
  pi({
    cwd: shop,
    id: ids["pi-nvidia"],
    model: "nvidia/GLM-5.3-Flash-NVFP4",
    prompt: "Profile the cart page and cut its load time.",
    provider: "modal-nv",
    start: "2026-11-16T06:00:00.000Z",
    turns: 45,
  }),
  pi({
    cwd: otherWt,
    id: ids["pi-othertool-worktree"],
    model: "zai-org/GLM-5.3-Flash",
    prompt: "Fix the retry backoff in the API client.",
    provider: "modal-glm",
    start: "2026-11-15T17:00:00.000Z",
    turns: 20,
  }),
];

// Each file's mtime is its last line's time, so the list order holds.
for (const f of files) {
  const file = path.join(home, f.file);

  write(file, f.text);
  utimesSync(file, f.mtime, f.mtime);
}

const dbFile = path.join(home, ".t3/userdata/statev2.sqlite");

mkdirSync(path.dirname(dbFile), { recursive: true });

writeT3Db(dbFile, { id: "proj-acme-shop", root: shop, title: "acme-shop" }, [
  {
    branch: "main",
    id: "thread-parent",
    nativeId: ids["t3code-parent"],
    title: "Fix checkout coupon validation",
    worktreePath: null,
  },
  {
    branch: childBranch,
    id: "thread-child",
    nativeId: ids["t3code-child-worktree"],
    parentId: "thread-parent",
    title: "Cover coupon edge cases in a worktree",
    worktreePath: childWt,
  },
]);

write(
  path.join(home, "media-manifest.json"),
  `${JSON.stringify({ rows: ids }, null, 2)}\n`
);

console.log(home);
