/**
 * Codex and Pi trees for the parity harness. `buildAgentsEdge` writes the
 * edge transcripts of `parity-codex-fixtures.ts` and `parity-pi-fixtures.ts`
 * under a fixture HOME (`.codex/sessions`, `.pi/agent/sessions`).
 * `buildPiRoot` copies the TS Pi fixtures under one slug dir.
 */
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  readdirSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { doc } from "./parity-agents-rows";
import { CODEX_LOCKED, codexFiles, meta } from "./parity-codex-fixtures";
import { chain, PI_LOCKED, piFiles, said, session } from "./parity-pi-fixtures";

const NO_ACCESS = 0o000;

const write = (path: string, body: string | Uint8Array) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
};

/** Codex and Pi edge trees under `home`; returns the files made unreadable,
 * which the caller restores. */
export const buildAgentsEdge = (home: string, extra: string) => {
  const codex = join(home, ".codex", "sessions");
  const pi = join(home, ".pi", "agent", "sessions");
  for (const [rel, body] of Object.entries(codexFiles)) {
    write(join(codex, rel), body);
  }
  for (const [rel, body] of Object.entries(piFiles)) {
    write(join(pi, rel), body);
  }
  mkdirSync(join(pi, "--tmp-demo--", "dir.jsonl"), { recursive: true });
  symlinkSync(join(codex, "2026", "01"), join(codex, "2026", "02"));
  symlinkSync(join(pi, "--other--"), join(pi, "--link--"));
  write(
    join(extra, "codex", "sessions", "rollout-extra.jsonl"),
    doc(meta(0, { id: "extra-codex", cwd: "/tmp/demo" }))
  );
  write(
    join(extra, "pi", "agent", "sessions", "--x--", "extra.jsonl"),
    doc(...chain(session({ id: undefined, cwd: "/tmp/demo" }), said("x")))
  );
  const locked = [join(codex, CODEX_LOCKED), join(pi, PI_LOCKED)];
  for (const path of locked) {
    write(path, doc(meta(0, { id: "locked" })));
    chmodSync(path, NO_ACCESS);
  }
  return locked;
};

/** `settings.roots` entries for Codex and Pi: the default again, a labelled
 * extra and a missing dir. */
export const agentsSettings = (extra: string, missing: string) => ({
  codex: [
    { path: "~/.codex" },
    { path: join(extra, "codex"), label: "cx" },
    { path: missing },
  ],
  pi: [
    { path: "~/.pi" },
    { path: join(extra, "pi"), label: "px" },
    { path: missing },
  ],
});

/** The TS Pi fixtures copied under one slug dir of a new root. */
export const buildPiRoot = (fixtures: string, root: string) => {
  const slug = join(root, "--tmp-pi--");
  mkdirSync(slug, { recursive: true });
  const src = join(fixtures, "sessions-pi");
  for (const name of readdirSync(src)) {
    copyFileSync(join(src, name), join(slug, name));
  }
  return root;
};
