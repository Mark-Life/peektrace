/** Serve-stage memory cases: reads on a shared HOME, and one write
 * sequence run on twin HOME copies, one per server. After the sequence the
 * two trees must match byte for byte. */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { report } from "./parity-lib";
import { tsValueSync } from "./parity-oracle";
import type { Counts } from "./parity-serve-cases";
import {
  type Call,
  compareCall,
  dropClock,
  exitDiff,
  type Norm,
  query,
  seg,
  send,
} from "./parity-serve-compare";
import {
  type ApiAnswer,
  api,
  type Pair,
  raw,
  replaceText,
  rpc,
} from "./parity-serve-lib";

const POLL_DEADLINE_MS = 3000;
const POLL_STEP_MS = 100;
const SEED = "-Users-demo-seed";

interface Project {
  readonly slug: string;
}

interface Entry {
  readonly mtimeMs: number;
  readonly slug: string;
}

const vaultPath = (slug: string) => `/api/memory/projects/${seg(slug)}`;
const entriesPath = (slug: string) => `${vaultPath(slug)}/entries`;

/** `memory.projects`, `memory.allVaults` and every vault, plus `extra`
 * slugs that are not projects with memory. */
export const memoryReadCases = async (
  c: Counts,
  pair: Pair,
  label: string,
  extra: readonly string[],
  retry = false
) => {
  const projects = await compareCall(
    c,
    pair,
    `${label} memory.projects`,
    { tag: "memory.projects", path: "/api/memory/projects" },
    { retry }
  );
  await compareCall(
    c,
    pair,
    `${label} memory.allVaults`,
    { tag: "memory.allVaults", path: "/api/memory" },
    { retry }
  );
  const listed =
    projects.exit.kind === "ok" ? (projects.exit.value as Project[]) : [];
  const slugs = [...new Set([...listed.map((p) => p.slug), ...extra])];
  for (const slug of slugs.filter((s) => s !== "")) {
    await compareCall(
      c,
      pair,
      `${label} memory.vault ${slug}`,
      {
        tag: "memory.vault",
        payload: { project: slug },
        path: vaultPath(slug),
      },
      { retry }
    );
  }
};

/** Mask what differs between two HOMEs: the HOME path and file times. */
const homeNorm =
  (homes: readonly string[]): Norm =>
  (v) => {
    const mapped = homes.reduce<unknown>(
      (acc, h) => replaceText(acc, h, "<HOME>"),
      v
    );
    const mask = (x: unknown): unknown => {
      if (Array.isArray(x)) {
        return x.map(mask);
      }
      if (x && typeof x === "object") {
        return Object.fromEntries(
          Object.entries(x).map(([k, y]) => [
            k,
            k === "mtime" || k === "mtimeMs" ? typeof y : mask(y),
          ])
        );
      }
      return x;
    };
    return mask(mapped);
  };

/** A Rust call the URL parser cannot express (a `..` segment), sent as
 * written over a socket. */
const rawApi = async (
  port: number,
  method: string,
  path: string,
  body: string
): Promise<ApiAnswer> => {
  const a = await raw(
    port,
    `${method} ${path} HTTP/1.1\r\nHost: 127.0.0.1:${port}\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${body}`
  );
  const type = a.headers["content-type"] ?? "";
  return {
    body: new TextEncoder().encode(a.body),
    json: type.startsWith("application/json") ? JSON.parse(a.body) : undefined,
    status: a.status,
    type,
  };
};

const ownMtime = async (pair: Pair, slug: string, name: string) => {
  const ts = await rpc(pair.ts.port, "memory.vault", { project: slug });
  const rust = await api(pair.rust.port, vaultPath(slug));
  const find = (v: unknown) =>
    (v as { entries?: Entry[] } | undefined)?.entries?.find(
      (e) => e.slug === name
    )?.mtimeMs;
  return {
    ts: ts.kind === "ok" ? find(ts.value) : undefined,
    rust: find(rust.json),
  };
};

type Payload = Record<string, unknown>;

const create = (project: string, fields: Payload): Call => ({
  tag: "memory.create",
  payload: { project, ...fields },
  path: entriesPath(project),
  init: send("POST", fields),
});

const update = (project: string, name: string, fields: Payload): Call => ({
  tag: "memory.update",
  payload: { project, name, ...fields },
  path: `${entriesPath(project)}/${seg(name)}`,
  init: send("PATCH", fields),
});

const remove = (project: string, name: string): Call => ({
  tag: "memory.delete",
  payload: { project, name },
  path: `${entriesPath(project)}/${seg(name)}`,
  init: { method: "DELETE" },
});

const note = (name: string, type = "reference") => ({
  name,
  description: "Third",
  type,
  body: "Gamma body.\n",
});

/** Every file under `root` with its bytes; temp files left by a failed
 * rename are skipped. */
export const tree = (root: string) => {
  const out = new Map<string, string>();
  const walk = (dir: string) => {
    for (const name of readdirSync(dir).sort()) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) {
        walk(path);
      } else if (!name.includes(".tmp-")) {
        out.set(relative(root, path), readFileSync(path).toString("base64"));
      }
    }
  };
  walk(root);
  return out;
};

export const sameTree = (label: string, a: string, b: string) => {
  const ta = new Map(tsValueSync(`${label} ts tree`, () => [...tree(a)]));
  const tb = tree(b);
  for (const k of new Set([...ta.keys(), ...tb.keys()])) {
    if (ta.get(k) !== tb.get(k)) {
      report(
        `serve ${label} tree: ${k} ${ta.has(k) ? "" : "missing in ts"}${tb.has(k) ? "" : "missing in rust"}${ta.has(k) && tb.has(k) ? "bytes differ" : ""}`
      );
    }
  }
};

/** The `..` create: Rust gets it as written, TS as a payload. */
const dotDotCreate = async (c: Counts, pair: Pair, norm: Norm) => {
  c.cases += 1;
  c.features += 1;
  const fields = note("okname", "user");
  const exit = await rpc(pair.ts.port, "memory.create", {
    project: "..",
    ...fields,
  });
  const r = await rawApi(
    pair.rust.port,
    "POST",
    "/api/memory/projects/../entries",
    JSON.stringify(fields)
  );
  for (const d of exitDiff(exit, r, norm)) {
    report(`serve memory.create ..: ${d}`);
  }
};

interface Versions {
  readonly memory: number;
  readonly sessions: number;
}

const watchVersions = async (pair: Pair) => {
  const t = await rpc(pair.ts.port, "watch.poll");
  const r = await api(pair.rust.port, "/api/watch");
  return {
    ts: t.kind === "ok" ? (t.value as Versions) : undefined,
    rust: r.json as Versions,
  };
};

/** One create raises each server's memory counter, and not its sessions. */
const watchBump = async (c: Counts, pair: Pair, norm: Norm) => {
  const before = await watchVersions(pair);
  await compareCall(
    c,
    pair,
    "memory.create watched",
    create(SEED, note("watched")),
    {
      norm,
    }
  );
  const deadline = Date.now() + POLL_DEADLINE_MS;
  let after = before;
  const bumped = () =>
    (after.ts?.memory ?? 0) >= (before.ts?.memory ?? 0) + 1 &&
    after.rust.memory >= before.rust.memory + 1;
  while (Date.now() < deadline) {
    after = await watchVersions(pair);
    if (bumped()) {
      break;
    }
    await Bun.sleep(POLL_STEP_MS);
  }
  c.cases += 1;
  c.features += 1;
  if (!bumped()) {
    report(
      `serve memory watch: no bump: ts ${JSON.stringify(before.ts)} -> ${JSON.stringify(after.ts)}, rust ${JSON.stringify(before.rust)} -> ${JSON.stringify(after.rust)}`
    );
  }
  if (
    after.ts?.sessions !== before.ts?.sessions ||
    after.rust.sessions !== before.rust.sessions
  ) {
    report("serve memory watch: sessions moved on a memory write");
  }
};

/** The write sequence; `homes` are the TS and Rust HOMEs, in that order. */
export const memoryWriteCases = async (
  c: Counts,
  pair: Pair,
  homes: { readonly ts: string; readonly rust: string }
) => {
  const norm = homeNorm([homes.ts, homes.rust]);
  const run = (label: string, call: Call) =>
    compareCall(c, pair, `memory ${label}`, call, { norm });
  await run("create", create(SEED, note("gamma")));
  await run("create dup", create(SEED, { ...note("gamma"), body: "x" }));
  await run("create bad name", create(SEED, note("Not Kebab", "user")));
  await run("create bad type", create(SEED, note("okname", "weird")));
  await run("create unknown project", create("nosuchproj", note("okname")));
  await dotDotCreate(c, pair, norm);
  await run("create no memory dir", create("-Users-demo-nomem", note("fresh")));
  await run("create missing field", create(SEED, { name: "x" }));
  const cas = await ownMtime(pair, SEED, "a");
  const body = { body: "New body.\n" };
  await run("update cas", {
    ...update(SEED, "a", { ...body, expectedMtime: cas.ts }),
    init: send("PATCH", { ...body, expectedMtime: cas.rust }),
  });
  await run(
    "update stale",
    update(SEED, "a", { body: "v3\n", expectedMtime: 1 })
  );
  await run(
    "update 1.5",
    update(SEED, "a", { body: "v3\n", expectedMtime: 1.5 })
  );
  await run(
    "update frontmatter",
    update(SEED, "b", { frontmatter: { description: "Changed", type: "user" } })
  );
  await run(
    "update bad type",
    update(SEED, "b", { frontmatter: { type: "weird" } })
  );
  await run("update not found", update(SEED, "nope", { body: "x" }));
  await run(
    "update escape",
    update("../../../../../../../tmp", "x", { body: "x" })
  );
  await run("update empty", update(SEED, "orphan", {}));
  await run(
    "update frontmatter {}",
    update(SEED, "fx-nested", { frontmatter: {} })
  );
  await run("update outside", update("../../outside", "x", { body: "y" }));
  await run("delete outside", remove("../../outside", "x"));
  await run("create outside", create("../../outside", note("zz", "user")));
  await run("delete", remove(SEED, "b"));
  await run("delete not found", remove(SEED, "nope"));
  await run("delete no index entry", remove("-Users-demo-intl", "ewe"));
  await watchBump(c, pair, norm);
  sameTree("memory-writes", homes.ts, homes.rust);
};

/** Under `--read-only` each write is refused with the same body; stats
 * still run. */
export const readOnlyFeatureCases = async (c: Counts, pair: Pair) => {
  const run = (label: string, call: Call) =>
    compareCall(c, pair, `read-only ${label}`, call);
  await run("create", create(SEED, note("rr", "user")));
  await run("create bad name", create(SEED, note("Bad Name", "user")));
  await run("update", update(SEED, "a", { body: "x" }));
  await run("delete", remove(SEED, "nope"));
  await run("stats.refresh", {
    tag: "stats.refresh",
    path: "/api/stats/refresh",
    init: send("POST", {}),
  });
  await compareCall(
    c,
    pair,
    "read-only stats.report",
    {
      tag: "stats.report",
      payload: { days: 7 },
      path: `/api/stats/report${query({ days: 7 })}`,
    },
    { norm: dropClock }
  );
};

/** Create, update (CAS ok and stale) and delete one probe memory in the
 * first real project with an index, on twin copies. */
export const realMemoryWrites = async (
  c: Counts,
  pair: Pair,
  homes: { readonly ts: string; readonly rust: string },
  slug: string
) => {
  const norm = homeNorm([homes.ts, homes.rust]);
  const run = (label: string, call: Call) =>
    compareCall(c, pair, `real memory ${label}`, call, { norm });
  await run("create", create(slug, note("parity-probe")));
  const cas = await ownMtime(pair, slug, "parity-probe");
  const body = { body: "Probe body.\n" };
  await run("update cas", {
    ...update(slug, "parity-probe", { ...body, expectedMtime: cas.ts }),
    init: send("PATCH", { ...body, expectedMtime: cas.rust }),
  });
  await run(
    "update stale",
    update(slug, "parity-probe", { body: "x\n", expectedMtime: 1 })
  );
  await run("delete", remove(slug, "parity-probe"));
  sameTree("real memory-writes", homes.ts, homes.rust);
};
