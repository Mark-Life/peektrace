/**
 * Host checks around the fixed goldens root: one run at a time, a root this
 * user owns, and the directory listing order goldens were recorded under.
 */
import {
  closeSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { join } from "node:path";

/** How a fresh directory lists its entries. Edge trees list symlinked
 * duplicates in this order, so record and replay hosts must agree. */
export type DirOrder = "creation" | "newest-first" | "other";

const PROBE_NAMES = ["c", "a", "b", "z", "m"];

export const orderOf = (
  created: readonly string[],
  listed: readonly string[]
) => {
  const same = (xs: readonly string[]) =>
    xs.length === listed.length && xs.every((x, i) => x === listed[i]);
  if (same(created)) {
    return "creation" satisfies DirOrder;
  }
  return same([...created].reverse()) ? "newest-first" : "other";
};

/** Lists a fresh dir under `root` to learn this host's order. */
export const probeDirOrder = (root: string) => {
  const dir = join(root, ".order-probe");
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  for (const name of PROBE_NAMES) {
    writeFileSync(join(dir, name), "");
  }
  const order = orderOf(PROBE_NAMES, readdirSync(dir));
  rmSync(dir, { recursive: true, force: true });
  return order;
};

/** Why `root` is unsafe to wipe, or undefined: it must be a real dir this
 * user owns, not a symlink someone else planted. */
export const rootProblem = (root: string, uid = process.getuid?.()) => {
  if (!existsSync(root)) {
    return;
  }
  const st = lstatSync(root);
  if (st.isSymbolicLink()) {
    return `${root} is a symlink; remove it`;
  }
  if (!st.isDirectory()) {
    return `${root} is not a directory; remove it`;
  }
  return uid !== undefined && st.uid !== uid
    ? `${root} belongs to uid ${st.uid}, not you (${uid})`
    : undefined;
};

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === "EPERM";
  }
};

/** Takes `<root>/.lock` for this process. A lock whose process is gone is
 * taken over. Returns the holder's pid when another live run has it. */
export const takeLock = (root: string, pid = process.pid) => {
  mkdirSync(root, { recursive: true, mode: 0o700 });
  const path = join(root, ".lock");
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      const fd = openSync(path, "wx");
      writeSync(fd, String(pid));
      closeSync(fd);
      return { release: () => rmSync(path, { force: true }) };
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== "EEXIST") {
        throw e;
      }
      const holder = Number(readFileSync(path, "utf8"));
      if (Number.isInteger(holder) && holder > 0 && alive(holder)) {
        return { holder };
      }
      rmSync(path, { force: true });
    }
  }
  return { holder: Number.NaN };
};
