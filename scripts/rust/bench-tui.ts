/** `peektrace tui` driver for `bench.sh`: writes a synthetic HOME, runs one
 * binary's TUI in a pseudo-terminal and prints its timings as JSON.
 *
 *   bun scripts/rust/bench-tui.ts home <dir> [turns] [body-bytes]
 *   bun scripts/rust/bench-tui.ts first <home> <runs> <warmup> -- <cmd...>
 *   bun scripts/rust/bench-tui.ts round <home> <filter> <presses> <idle-s> -- <cmd...>
 *
 * `first` times spawn to the first full frame. `round` also opens the
 * session the filter matches (or the second newest when the filter is `-`),
 * presses `j` in its history, then idles with the watcher on. A full frame
 * is a screen holding the session list title and a history header. */
import {
  mkdirSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { createScreen } from "./bench-tui-screen";

const COLS = 160;
const ROWS = 48;
/** Output gap that ends a frame burst. */
const QUIET_MS = 40;
const TIMEOUT_MS = 30_000;
const SMALL_SESSIONS = 30;
const DAY_MS = 86_400_000;
const BIG_START = Date.parse("2026-11-10T09:00:00.000Z");

const uuid = (n: number) =>
  `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const iso = (ms: number) => new Date(ms).toISOString();

/** Tool result text of about `bytes` bytes: TypeScript-looking lines. */
const body = (i: number, bytes: number) => {
  const lines: string[] = [];
  let size = 0;
  for (let n = 0; size < bytes; n++) {
    const l = `export const value${i}_${n} = (a: number, b: string) => \`\${a} and \${b}\`; // line ${n}`;
    lines.push(l);
    size += l.length + 1;
  }
  return lines.join("\n");
};

const bigSession = (turns: number, bodyBytes: number) => {
  const cwd = "/Users/bench/big";
  const out: string[] = [];
  const push = (v: unknown) => out.push(JSON.stringify(v));
  let t = BIG_START;
  push({
    type: "user",
    cwd,
    gitBranch: "main",
    version: "2.0.0",
    sessionId: uuid(1),
    timestamp: iso(t),
    uuid: "u-0",
    message: { role: "user", content: "Port the parser and keep tests green." },
  });
  for (let i = 0; i < turns; i++) {
    t += 2000;
    const tool = `toolu_${i}`;
    push({
      type: "assistant",
      cwd,
      sessionId: uuid(1),
      requestId: `req_${i}`,
      timestamp: iso(t),
      uuid: `a-${i}`,
      message: {
        id: `msg_${i}`,
        role: "assistant",
        model: "claude-sonnet-4-5",
        usage: {
          input_tokens: 10,
          cache_read_input_tokens: 4000 + (i % 400) * 400,
          output_tokens: 120,
        },
        content: [
          ...(i % 5 === 0
            ? [{ type: "thinking", thinking: `Plan for step ${i}.` }]
            : []),
          { type: "text", text: `Step ${i}: reading \`src/mod${i}.ts\`.` },
          {
            type: "tool_use",
            id: tool,
            name: i % 3 === 0 ? "Bash" : "Read",
            input:
              i % 3 === 0
                ? { command: `bun test src/mod${i}.test.ts 2>&1 | tail -40` }
                : { file_path: `${cwd}/src/mod${i}.ts` },
          },
        ],
      },
    });
    t += 1000;
    push({
      type: "user",
      cwd,
      sessionId: uuid(1),
      timestamp: iso(t),
      uuid: `r-${i}`,
      message: {
        role: "user",
        content: [
          {
            type: "tool_result",
            tool_use_id: tool,
            content: body(i, bodyBytes),
          },
        ],
      },
    });
    if (i % 50 === 49) {
      t += 1000;
      push({
        type: "user",
        cwd,
        sessionId: uuid(1),
        timestamp: iso(t),
        uuid: `p-${i}`,
        message: { role: "user", content: `Continue with batch ${i + 1}.` },
      });
    }
  }
  return { text: `${out.join("\n")}\n`, end: t };
};

const smallSession = (n: number, at: number) => {
  const cwd = `/Users/bench/small${n}`;
  const line = (v: unknown) => JSON.stringify(v);
  return `${[
    line({
      type: "user",
      cwd,
      sessionId: uuid(100 + n),
      timestamp: iso(at),
      uuid: "u-0",
      message: { role: "user", content: `Small task ${n}` },
    }),
    line({
      type: "assistant",
      cwd,
      sessionId: uuid(100 + n),
      requestId: "req_0",
      timestamp: iso(at + 1000),
      uuid: "a-0",
      message: {
        id: "msg_0",
        role: "assistant",
        model: "claude-sonnet-4-5",
        usage: { input_tokens: 10, output_tokens: 20 },
        content: [{ type: "text", text: `Done with task ${n}.` }],
      },
    }),
  ].join("\n")}\n`;
};

/** HOME with one big Claude session, second newest, and small ones around
 * it; the newest small session is selected on start. */
const writeHome = (dir: string, turns: number, bodyBytes: number) => {
  rmSync(dir, { recursive: true, force: true });
  const projects = join(dir, ".claude", "projects");
  const put = (slug: string, id: string, text: string, mtime: number) => {
    const p = join(projects, slug, `${id}.jsonl`);
    mkdirSync(join(projects, slug), { recursive: true });
    writeFileSync(p, text);
    utimesSync(p, mtime / 1000, mtime / 1000);
  };
  const big = bigSession(turns, bodyBytes);
  put("-Users-bench-big", uuid(1), big.text, big.end);
  for (let n = 0; n < SMALL_SESSIONS; n++) {
    const at = n === 0 ? big.end + DAY_MS : BIG_START - n * DAY_MS;
    put(
      `-Users-bench-small${n}`,
      uuid(100 + n),
      smallSession(n, at),
      at + 1000
    );
  }
  mkdirSync(join(dir, ".peektrace"), { recursive: true });
  console.log(
    JSON.stringify({ bytes: Buffer.byteLength(big.text), turns, bodyBytes })
  );
};

// --- driving a TUI ------------------------------------------------------------

/** No frame within this long after a key counts as a miss. */
const PRESS_TIMEOUT_MS = 3000;
/** Scrolling stops after this many misses in a row. */
const MAX_MISSES = 3;
const WARM_PRESSES = 5;
const SETTLE_MS = 500;
const PLATFORM = process.platform;

const LIST = /Sessions \((\d+)\/(\d+)\)/;
const EVENTS = /(\d[\d,]*) events ·/;
const eventsOf = (text: string) => {
  const m = EVENTS.exec(text);
  return m === null ? undefined : Number(m[1].replaceAll(",", ""));
};
/** The list is loaded and the selected session's history is drawn. */
const fullFrame = (text: string) => {
  const list = LIST.exec(text);
  return (
    list !== null &&
    Number(list[2]) > 0 &&
    eventsOf(text) !== undefined &&
    !text.includes("Analyzing")
  );
};

const sh = (cmd: string[]) => Bun.spawnSync(cmd).stdout.toString().trim();

const rssKiB = (pid: number) =>
  Number(sh(["ps", "-o", "rss=", "-p", String(pid)])) || 0;

/** User plus system CPU of `pid` in ms: `/proc` on Linux (ps rounds to
 * seconds there), `ps -o time=` (centiseconds) on macOS. */
const cpuMs = (pid: number) => {
  if (PLATFORM === "linux") {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const f = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    const tick = Number(sh(["getconf", "CLK_TCK"])) || 100;
    return ((Number(f[11]) + Number(f[12])) * 1000) / tick;
  }
  const parts = sh(["ps", "-o", "time=", "-p", String(pid)])
    .split(":")
    .map(Number);
  return parts.reduce((acc, v) => acc * 60 + v, 0) * 1000;
};

const launch = (cmd: string[], env: Record<string, string>) => {
  const screen = createScreen(COLS, ROWS);
  let last = 0;
  let chunks = 0;
  let wake: (() => void) | undefined;
  const t0 = performance.now();
  const proc = Bun.spawn(cmd, {
    env,
    terminal: {
      cols: COLS,
      rows: ROWS,
      data: (_term, data) => {
        screen.write(data);
        last = performance.now();
        chunks++;
        wake?.();
      },
    },
  });
  const nap = (ms: number) =>
    new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, ms);
      wake = () => {
        clearTimeout(timer);
        resolve();
      };
    });
  /** Waits until `pred` holds on the screen; false on timeout. */
  const until = async (pred: (text: string) => boolean, ms: number) => {
    const end = performance.now() + ms;
    while (!pred(screen.text())) {
      const left = end - performance.now();
      if (left <= 0) {
        return false;
      }
      await nap(left);
    }
    return true;
  };
  /** Time of the last byte before `QUIET_MS` without output. */
  const settle = async () => {
    for (;;) {
      const gap = performance.now() - last;
      if (gap >= QUIET_MS) {
        return last;
      }
      await Bun.sleep(QUIET_MS - gap);
    }
  };
  /** Sends `key`; ms from the write to the end of the frame it causes, or
   * undefined when nothing is drawn. */
  const press = async (key: string) => {
    const before = chunks;
    const at = performance.now();
    proc.terminal?.write(key);
    const drawn = await until(() => chunks > before, PRESS_TIMEOUT_MS);
    return drawn ? (await settle()) - at : undefined;
  };
  const quit = async (pid: number) => {
    proc.terminal?.write("q");
    const exited = await Promise.race([
      proc.exited.then(() => true),
      Bun.sleep(5000).then(() => false),
    ]);
    if (!exited) {
      process.kill(pid, "SIGKILL");
      await proc.exited;
    }
    proc.terminal?.close();
    return exited ? "q" : "killed";
  };
  return { press, proc, quit, screen, settle, t0, until };
};

/** The TUI's pid: `cmd[0]` itself, or its child when wrapped in GNU time. */
const tuiPid = async (pid: number, wrapped: boolean) => {
  for (let i = 0; wrapped && i < 200; i++) {
    const child = Number(sh(["pgrep", "-P", String(pid)]).split("\n")[0]);
    if (child > 0) {
      return child;
    }
    await Bun.sleep(5);
  }
  return pid;
};

const tuiEnv = (home: string, dir: string) => ({
  PATH: "/usr/bin:/bin",
  HOME: home,
  TZ: "UTC",
  TERM: "xterm-256color",
  COLORTERM: "truecolor",
  PEEKTRACE_DIR: dir,
  PEEKTRACE_NO_UPDATE_CHECK: "1",
});

/** Spawn to the end of the first full frame, then quit. */
const first = async (home: string, dir: string, bin: string[]) => {
  const t = launch(bin, tuiEnv(home, dir));
  const ok = await t.until(fullFrame, TIMEOUT_MS);
  const end = await t.settle();
  const pid = await tuiPid(t.proc.pid, false);
  const quit = await t.quit(pid);
  return { firstMs: ok ? end - t.t0 : null, quit };
};

/** Selects the target session: `next` presses `j` once; `filter:<text>`
 * types `/<text>` then Esc. Ms from the last key to the end of the target's
 * history frame. */
const open = async (
  t: ReturnType<typeof launch>,
  how: string,
  from: number
) => {
  const opened = (text: string) => {
    const n = eventsOf(text);
    return n !== undefined && n !== from && !text.includes("Analyzing");
  };
  let at: number;
  if (how === "next") {
    at = performance.now();
    t.proc.terminal?.write("j");
  } else {
    t.proc.terminal?.write("/");
    await Bun.sleep(100);
    const chars = [...how.slice("filter:".length)];
    for (const ch of chars.slice(0, -1)) {
      t.proc.terminal?.write(ch);
      await Bun.sleep(30);
    }
    at = performance.now();
    t.proc.terminal?.write(chars.at(-1) ?? "");
  }
  const ok = await t.until(opened, TIMEOUT_MS);
  const end = await t.settle();
  if (how !== "next") {
    t.proc.terminal?.write("\x1b");
    await Bun.sleep(SETTLE_MS);
    await t.settle();
  }
  return ok ? end - at : null;
};

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

/** One full round under GNU time: first frame, open the target, Enter into
 * its history, `j` presses, idle with the watcher on, quit. */
const round = async (args: {
  bin: string[];
  dir: string;
  gtime: string;
  home: string;
  how: string;
  idleS: number;
  presses: number;
  rssFile: string;
}) => {
  const cmd = [args.gtime, "-f", "%M", "-o", args.rssFile, ...args.bin];
  const t = launch(cmd, tuiEnv(args.home, args.dir));
  const pid = await tuiPid(t.proc.pid, true);
  const firstOk = await t.until(fullFrame, TIMEOUT_MS);
  const firstMs = firstOk ? (await t.settle()) - t.t0 : null;
  const from = eventsOf(t.screen.text()) ?? -1;
  const openMs = firstOk ? await open(t, args.how, from) : null;
  const events = eventsOf(t.screen.text());
  /** Set when the TUI wiped the screen instead of drawing the session. */
  const blank = t.screen.text().trim() === "";
  const enterMs = openMs === null ? undefined : await t.press("\r");
  await Bun.sleep(SETTLE_MS);
  const rssOpenKiB = rssKiB(pid);
  const scrollMs: number[] = [];
  let misses = 0;
  let missed = 0;
  for (
    let i = 0;
    openMs !== null && i < WARM_PRESSES + args.presses && misses < MAX_MISSES;
    i++
  ) {
    const ms = await t.press("j");
    if (ms === undefined) {
      misses++;
      missed++;
    } else {
      misses = 0;
      if (i >= WARM_PRESSES) {
        scrollMs.push(ms);
      }
    }
    await Bun.sleep(60);
  }
  await Bun.sleep(2000);
  const c0 = cpuMs(pid);
  await Bun.sleep(args.idleS * 1000);
  const idleCpuMs = cpuMs(pid) - c0;
  const quit = await t.quit(pid);
  const peak = readFileSync(args.rssFile, "utf8").trim().split("\n").at(-1);
  return {
    firstMs,
    openMs,
    enterMs: enterMs ?? null,
    events: events ?? null,
    blank,
    rssOpenKiB,
    scrollMs,
    scrollMedianMs: scrollMs.length > 0 ? median(scrollMs) : null,
    scrollMissed: missed,
    idleS: args.idleS,
    idleCpuMs,
    peakKiB: Number(peak) || null,
    quit,
  };
};

const usage = () => {
  console.error(
    "usage: bench-tui.ts home <dir> <turns> <body-bytes>\n" +
      "       bench-tui.ts first <home> <peektrace-dir> -- <cmd...>\n" +
      "       bench-tui.ts round <home> <peektrace-dir> <next|filter:text> <presses> <idle-s> <gnu-time> <rss-file> -- <cmd...>"
  );
  process.exit(2);
};

const main = async () => {
  const argv = process.argv.slice(2);
  const dash = argv.indexOf("--");
  const [mode, ...rest] = dash < 0 ? argv : argv.slice(0, dash);
  const bin = dash < 0 ? [] : argv.slice(dash + 1);
  if (mode === "home" && rest.length === 3) {
    writeHome(rest[0], Number(rest[1]), Number(rest[2]));
  } else if (mode === "first" && rest.length === 2 && bin.length > 0) {
    console.log(JSON.stringify(await first(rest[0], rest[1], bin)));
  } else if (mode === "round" && rest.length === 7 && bin.length > 0) {
    const [home, dir, how, presses, idleS, gtime, rssFile] = rest;
    console.log(
      JSON.stringify(
        await round({
          bin,
          dir,
          gtime,
          home,
          how,
          idleS: Number(idleS),
          presses: Number(presses),
          rssFile,
        })
      )
    );
  } else {
    usage();
  }
};

await main();
