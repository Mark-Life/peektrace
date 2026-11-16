/** `upgrade` parity against a local fake release server; never the real
 * GitHub.
 *
 * - `--check` cases and `install-same` run both CLIs and compare stdout,
 *   stderr, exit and the request log. Bun-specific failure causes (bad JSON,
 *   connection refused) are compared up to the message prefix.
 * - `install-same` runs a real `upgrade`. It is safe only while both CLIs
 *   report the dev version, which equals the fake `cli-v0.0.0` tag; else TS
 *   would replace its running binary (`bun` from source), so the case is
 *   skipped. Rust always gets a temp `PEEKTRACE_UPGRADE_TARGET`.
 * - The install route compares TS `performUpgrade` on a temp target (run
 *   with `bun -e` from `apps/cli`) with Rust `upgrade --version cli-v9.9.9`
 *   and `PEEKTRACE_UPGRADE_TARGET`: the error text, the target bytes and
 *   mode, the dir listing and the request paths. */
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import {
  compareRun,
  REPO,
  type Run,
  report,
  runAsync,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { bind, tsValue, tsValueSync } from "./parity-oracle";

const PAYLOAD = new Uint8Array([0x70, 0x65, 0x65, 0x6b, 9, 9, 9]);
const HEX = createHash("sha256").update(PAYLOAD).digest("hex");
const OLD = new Uint8Array([1, 2, 3]);
const TAG = "cli-v9.9.9";
const PREFIX_OF = /^(Failed to query GitHub releases: )/;
const ERROR_PREFIX = /^error /;

const hostAsset = () => {
  const os = { darwin: "darwin", linux: "linux", win32: "windows" }[
    process.platform as string
  ];
  const arch = process.arch === "arm64" ? "arm64" : "x64";
  return `peektrace-${os}-${arch}${os === "windows" ? ".exe" : ""}`;
};

const notFound = () => new Response("nf", { status: 404 });

const RELEASES: Record<string, () => Response> = {
  403: () => new Response("rate limited", { status: 403 }),
  none: () => Response.json([{ tag_name: "desktop-v1.0.0" }]),
  same: () => Response.json([{ tag_name: "cli-v0.0.0" }]),
  obj: () => Response.json({ message: "x" }),
  badjson: () => new Response("{not json"),
};

const releases = (mode: string) =>
  RELEASES[mode]?.() ??
  Response.json([
    { tag_name: "desktop-v3.0.0" },
    { tag_name: TAG },
    { tag_name: "cli-v0.0.1" },
  ]);

const sums = (mode: string, asset: string) => {
  if (mode === "nosum") {
    return new Response(`${HEX}  other-asset\n`);
  }
  if (mode === "nosums404") {
    return notFound();
  }
  const hex = mode === "wrong" ? "0".repeat(64) : HEX.toUpperCase();
  return new Response(`${hex}  ${asset}\n`);
};

const gzAsset = (mode: string) => {
  if (mode === "badgz") {
    return new Response("not gzip");
  }
  return mode === "nogz" || mode === "noasset"
    ? notFound()
    : new Response(gzipSync(PAYLOAD));
};

const route = (path: string, mode: string, asset: string) => {
  if (path === "/releases") {
    return releases(mode);
  }
  if (path.endsWith("/SHA256SUMS")) {
    return sums(mode, asset);
  }
  if (path.endsWith(".gz")) {
    return gzAsset(mode);
  }
  if (path.endsWith(`/${asset}`) && mode !== "noasset") {
    return new Response(PAYLOAD);
  }
  return notFound();
};

/** The fake server: `mode` picks each route's answer; `log` records calls. */
const fakeServer = (asset: string) => {
  const state = { mode: "ok", log: [] as string[] };
  const server = Bun.serve({
    port: 0,
    hostname: "127.0.0.1",
    fetch(req) {
      const u = new URL(req.url);
      const headers =
        u.pathname === "/releases"
          ? ` ua=${req.headers.get("user-agent")} accept=${req.headers.get("accept")}`
          : "";
      state.log.push(`${req.method} ${u.pathname}${u.search}${headers}`);
      return route(u.pathname, state.mode, asset);
    },
  });
  return { server, state, origin: `http://127.0.0.1:${server.port}` };
};

type Fake = ReturnType<typeof fakeServer>;

const envFor = (
  home: string,
  origin: string,
  extra: Record<string, string> = {}
) => ({
  PATH: process.env.PATH ?? "",
  HOME: home,
  PEEKTRACE_DIR: join(home, ".peektrace"),
  PEEKTRACE_NO_TELEMETRY: "1",
  PEEKTRACE_GITHUB_API: origin,
  PEEKTRACE_BASE_URL: origin,
  ...extra,
});

interface CheckCase {
  readonly args: readonly string[];
  readonly env?: Record<string, string>;
  /** Runs a real `upgrade`: needs the dev-version guard. */
  readonly installs?: boolean;
  readonly mode: string;
  readonly name: string;
  /** Compare stderr only up to this prefix (Bun-specific causes). */
  readonly prefix?: boolean;
}

const CHECK_CASES: readonly CheckCase[] = [
  { name: "check-newer", mode: "ok", args: ["upgrade", "--check"] },
  { name: "check-same", mode: "same", args: ["upgrade", "--check"] },
  { name: "check-none", mode: "none", args: ["upgrade", "--check"] },
  { name: "check-403", mode: "403", args: ["upgrade", "--check"] },
  { name: "check-obj", mode: "obj", args: ["upgrade", "--check"] },
  {
    name: "check-badjson",
    mode: "badjson",
    args: ["upgrade", "--check"],
    prefix: true,
  },
  {
    name: "check-pin-flag",
    mode: "ok",
    args: ["upgrade", "--check", "--version", "1.2.3"],
  },
  {
    name: "check-pin-older",
    mode: "ok",
    args: ["upgrade", "--check", "--version", "cli-v0.0.0"],
  },
  {
    name: "check-pin-dev",
    mode: "ok",
    args: ["upgrade", "--check", "--version", "0.0.0"],
  },
  {
    name: "check-pin-env",
    mode: "ok",
    args: ["upgrade", "--check"],
    env: { PEEKTRACE_VERSION: "cli-v0.0.0" },
  },
  {
    name: "check-pin-env-newer",
    mode: "ok",
    args: ["upgrade", "--check"],
    env: { PEEKTRACE_VERSION: "v2.0.0" },
  },
  {
    name: "check-pin-env-empty",
    mode: "ok",
    args: ["upgrade", "--check"],
    env: { PEEKTRACE_VERSION: "" },
  },
  {
    name: "check-noupd",
    mode: "ok",
    args: ["upgrade", "--check"],
    env: { PEEKTRACE_NO_UPDATE_CHECK: "1" },
  },
  { name: "check-json", mode: "ok", args: ["--json", "upgrade", "--check"] },
  {
    name: "check-remote",
    mode: "ok",
    args: ["--remote", "http://x", "upgrade", "--check"],
  },
  {
    name: "check-unreachable",
    mode: "ok",
    args: ["upgrade", "--check"],
    env: { PEEKTRACE_GITHUB_API: "http://127.0.0.1:1" },
    prefix: true,
  },
  { name: "install-same", mode: "same", args: ["upgrade"], installs: true },
];

const DEV_VERSION = "0.0.0-dev";

/** Both CLIs report the dev version, so `upgrade` stops at "Already on the
 * latest version" instead of replacing a binary. */
const bothDev = async (env: Record<string, string>) => {
  const [ts, rust] = await Promise.all([
    runAsync([...TS_CMD, "--version"], env),
    runAsync([rustBin, "--version"], env),
  ]);
  return ts.stdout.trim() === DEV_VERSION && rust.stdout.trim() === DEV_VERSION;
};

/** A temp dir holding the old binary; returns the target path. */
const target = (side: string, mode: string) => {
  const dir = join(WORK, "upgrade", `${side}-${mode}`);
  mkdirSync(dir, { recursive: true });
  const path = join(dir, "peektrace");
  writeFileSync(path, OLD, { mode: 0o644 });
  return path;
};

const snapshot = (path: string) => {
  const dir = join(path, "..");
  return {
    bytes: [...readFileSync(path)].join(","),
    mode: statSync(path).mode.toString(8).slice(-3),
    files: readdirSync(dir).sort().join(","),
  };
};

const cut = (r: Run) => ({
  ...r,
  stderr: r.stderr.match(PREFIX_OF)?.[1] ?? r.stderr,
});

const runChecks = async (fake: Fake, home: string) => {
  const dev = await bothDev(envFor(home, fake.origin));
  for (const c of CHECK_CASES) {
    if (c.installs && !dev) {
      console.log(`upgrade ${c.name}: skipped, not a ${DEV_VERSION} build`);
      continue;
    }
    const env = envFor(home, fake.origin, c.env);
    fake.state.mode = c.mode;
    fake.state.log = [];
    const ts = await runAsync([...TS_CMD, ...c.args], env);
    const tsLog = tsValueSync(`upgrade ${c.name} ts requests`, () =>
      fake.state.log.join("\n")
    );
    fake.state.log = [];
    const rustTarget = c.installs ? target("rust", c.name) : undefined;
    const rust = await runAsync(
      [rustBin, ...c.args],
      rustTarget ? { ...env, PEEKTRACE_UPGRADE_TARGET: rustTarget } : env
    );
    const rustLog = fake.state.log.join("\n");
    compareRun(
      `upgrade ${c.name}`,
      c.prefix ? cut(ts) : ts,
      c.prefix ? cut(rust) : rust
    );
    if (rustTarget && snapshot(rustTarget).bytes !== OLD.join(",")) {
      report(`upgrade ${c.name}: rust replaced its target`);
    }
    if (tsLog !== rustLog) {
      report(
        `upgrade ${c.name}: requests ts=${JSON.stringify(tsLog)} rust=${JSON.stringify(rustLog)}`
      );
    }
  }
  return CHECK_CASES.length;
};

const INSTALL_MODES = [
  "ok",
  "wrong",
  "nosum",
  "nosums404",
  "badgz",
  "nogz",
  "noasset",
];

const tsPerform = async (
  asset: string,
  path: string,
  env: Record<string, string>
) => {
  const code = `
    const { performUpgrade } = await import(${JSON.stringify(join(REPO, "apps/cli/src/upgrade/install.ts"))});
    const { resolveReleaseConfig } = await import(${JSON.stringify(join(REPO, "apps/cli/src/upgrade/release.ts"))});
    const { Effect, Either } = await import("effect");
    const r = await Effect.runPromise(Effect.either(performUpgrade({ config: resolveReleaseConfig(), tag: ${JSON.stringify(TAG)}, asset: ${JSON.stringify(asset)}, targetPath: ${JSON.stringify(path)} })));
    console.log(Either.isRight(r) ? "bytes " + r.right.bytes : "error " + r.left.message);
  `;
  const proc = Bun.spawn(["bun", "-e", code], {
    env,
    cwd: join(REPO, "apps/cli"),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [out, err] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { out: out.trim(), err };
};

const runInstalls = async (fake: Fake, home: string, asset: string) => {
  for (const mode of INSTALL_MODES) {
    fake.state.mode = mode;
    fake.state.log = [];
    const tsPath = target("ts", mode);
    const { ts, tsLog, a } = await tsValue(
      `upgrade install-${mode}`,
      async () => ({
        ts: await tsPerform(asset, tsPath, envFor(home, fake.origin)),
        tsLog: fake.state.log.join("\n"),
        a: snapshot(tsPath),
      })
    );
    fake.state.log = [];
    const rustPath = target("rust", mode);
    const rust = await runAsync(
      [rustBin, "upgrade", "--version", TAG],
      envFor(home, fake.origin, { PEEKTRACE_UPGRADE_TARGET: rustPath })
    );
    const rustLog = fake.state.log.join("\n");
    const label = `upgrade install-${mode}`;
    const expected = ts.out.startsWith("bytes ")
      ? {
          code: 0,
          stdout: `Fetching ${asset} (${TAG})...\nUpgraded to ${TAG} (downloaded ${ts.out.slice(6)} B). It takes effect on the next launch.\n`,
          stderr: "",
        }
      : {
          code: 1,
          stdout: `Fetching ${asset} (${TAG})...\n`,
          stderr: `${ts.out.replace(ERROR_PREFIX, "")}\n`,
        };
    if (ts.err !== "") {
      report(`${label}: ts driver failed: ${ts.err.slice(0, 200)}`);
    }
    compareRun(label, expected, rust);
    const b = snapshot(rustPath);
    for (const k of ["bytes", "mode", "files"] as const) {
      if (a[k] !== b[k]) {
        report(`${label}: ${k} ts=${a[k]} rust=${b[k]}`);
      }
    }
    if (tsLog !== rustLog) {
      report(
        `${label}: requests ts=${JSON.stringify(tsLog)} rust=${JSON.stringify(rustLog)}`
      );
    }
  }
  return INSTALL_MODES.length;
};

export const runUpgradeStage = async () => {
  const asset = hostAsset();
  const fake = fakeServer(asset);
  bind("<ORIGIN>", fake.origin);
  bind("<ASSET>", asset);
  const home = join(WORK, "upgrade-home");
  mkdirSync(home, { recursive: true });
  try {
    const checks = await runChecks(fake, home);
    const installs = await runInstalls(fake, home, asset);
    console.log(
      `suite upgrade: ${checks} check cases, ${installs} install cases`
    );
  } finally {
    fake.server.stop(true);
  }
};
