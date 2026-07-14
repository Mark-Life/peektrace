/**
 * Sidecar lifecycle manager run inside the Electrobun Bun main process.
 *
 * The shell picks a free loopback port, spawns the `peektrace` server on it, and
 * waits for the machine-readable `PEEKTRACE_READY:<port>` line on stdout before
 * navigating the window. The port on the READY line is authoritative — the
 * server may land on a different port than requested. Human-readable log text is
 * never part of the startup contract.
 *
 * In dev the sidecar is `bun run apps/cli/src/index.ts serve`; a packaged build
 * runs the bundled compiled binary staged under the app's Resources.
 */
import { type ChildProcess, spawn } from "node:child_process";
import { chmodSync, existsSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PATHS } from "electrobun/bun";
import { scoped } from "./log";

const sidecarLog = scoped("sidecar");

const STDERR_TAIL_LIMIT = 8192;
const READY_SENTINEL = "PEEKTRACE_READY";
const NEWLINE_SPLIT = /\r?\n/;
const PORT_IN_USE_PATTERN = /EADDRINUSE|address already in use/i;
const HOST = "127.0.0.1";
const STOP_GRACE_MS = 5000;
const HEALTH_TIMEOUT_MS = 1500;
const HEALTH_ATTEMPTS = 3;
const HEALTH_RETRY_DELAY_MS = 150;
const DECIMAL = 10;
const EXEC_MODE = 0o755;

// Children deliberately stopped via stopSidecar (quit, restart) — their exits
// are expected and must not surface as crashes.
const expectedExits = new WeakSet<ChildProcess>();

// index.ts subscribes to swap the dead web UI for the crash screen. A callback
// (not an import) keeps this module free of window concerns.
let unexpectedExitListener: (() => void) | null = null;

/** Register the callback fired when a live sidecar exits without being asked to. */
export const onUnexpectedSidecarExit = (listener: () => void) => {
  unexpectedExitListener = listener;
};

/** Buffer chunked output into whole lines before handing them to `write`. */
const makeLineSplitter = (write: (line: string) => void) => {
  let buffer = "";
  return (text: string) => {
    buffer += text;
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length > 0) {
        write(line);
      }
    }
  };
};

export interface SidecarConnection {
  readonly baseUrl: string;
  readonly child: ChildProcess;
  readonly hostname: string;
  readonly port: number;
}

export class SidecarPortInUseError extends Error {
  readonly port: number;
  constructor(port: number) {
    super(`Port ${port} is already in use.`);
    this.name = "SidecarPortInUseError";
    this.port = port;
  }
}

const delay = (ms: number): Promise<void> =>
  new Promise((resolveDelay) => {
    setTimeout(resolveDelay, ms);
  });

/** Ask the OS for a free loopback TCP port by binding `:0` and reading it back. */
const pickFreePort = (): Promise<number> =>
  new Promise((resolvePort, rejectPort) => {
    const probe = createServer();
    probe.once("error", (error) => {
      probe.close();
      rejectPort(error);
    });
    probe.listen(0, HOST, () => {
      const address = probe.address();
      const port = typeof address === "object" && address ? address.port : 0;
      probe.close(() => resolvePort(port));
    });
  });

const binaryName = (): string =>
  process.platform === "win32" ? "peektrace.exe" : "peektrace";

/**
 * The bundled sidecar binary is copied to `<Resources>/app/peektrace/` by the
 * electrobun `copy` step. RESOURCES_FOLDER is derived from the process cwd, which
 * is not guaranteed inside every launcher, so a few import-relative fallbacks are
 * probed too. Returns the first path that exists, or null in dev (unstaged).
 */
const findPackagedBinary = (): string | null => {
  const name = binaryName();
  const here = dirname(fileURLToPath(import.meta.url));
  const candidates = [
    join(PATHS.RESOURCES_FOLDER, "app", "peektrace", name),
    join(here, "..", "peektrace", name),
    join(here, "peektrace", name),
    join(process.cwd(), "peektrace", name),
  ];
  return candidates.find((path) => existsSync(path)) ?? null;
};

/**
 * Walk up from a starting directory until the repo's CLI entry is found. Used in
 * dev to locate `apps/cli/src/index.ts` regardless of the launcher's cwd.
 */
const findRepoRoot = (): string => {
  const marker = join("apps", "cli", "src", "index.ts");
  const starts = [process.cwd(), dirname(fileURLToPath(import.meta.url))];
  for (const start of starts) {
    let dir = start;
    for (let depth = 0; depth < 10; depth += 1) {
      if (existsSync(join(dir, marker))) {
        return dir;
      }
      const parent = dirname(dir);
      if (parent === dir) {
        break;
      }
      dir = parent;
    }
  }
  return process.cwd();
};

interface SidecarCommand {
  readonly args: readonly string[];
  readonly command: string;
  readonly cwd: string;
}

/**
 * Resolve the `[command, args, cwd]` to launch the sidecar on `port`. When a
 * bundled binary is present (packaged build) it is run directly; otherwise dev
 * mode runs the CLI TypeScript entry through `bun`.
 */
const resolveSidecar = (port: number): SidecarCommand => {
  const packaged =
    process.env.PEEKTRACE_DESKTOP_DEV === "1" ? null : findPackagedBinary();
  if (packaged) {
    // The exec bit is preserved end-to-end (stage chmod → copy → tar → extract),
    // so this is only a belt for odd extractions. Skip it on macOS: electrobun's
    // extractor deliberately avoids chmod on `.app` bundles to keep code
    // signatures intact, and we should not diverge once signing is enabled.
    if (process.platform !== "darwin") {
      try {
        chmodSync(packaged, EXEC_MODE);
      } catch {
        // The copy step already preserves the exec bit; ignore failures.
      }
    }
    return {
      command: packaged,
      args: ["serve", "--port", String(port), "--no-open"],
      cwd: dirname(packaged),
    };
  }
  const repoRoot = findRepoRoot();
  const cliEntry = resolve(repoRoot, "apps", "cli", "src", "index.ts");
  return {
    command: "bun",
    args: ["run", cliEntry, "serve", "--port", String(port), "--no-open"],
    cwd: repoRoot,
  };
};

/** Spawn the sidecar and resolve once it announces `PEEKTRACE_READY:<port>`. */
export const startSidecar = async (): Promise<SidecarConnection> => {
  const port = await pickFreePort();
  const { command, args, cwd } = resolveSidecar(port);

  const child = spawn(command, args, {
    cwd,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      // Signals the server to print the PEEKTRACE_READY:<port> handshake line.
      PEEKTRACE_CLIENT: "desktop",
    },
  });

  return new Promise<SidecarConnection>((resolveStart, rejectStart) => {
    let stderrBuffer = "";
    let stdoutControlBuffer = "";
    let resolved = false;
    let rejected = false;

    const logStdoutLine = makeLineSplitter((line) => sidecarLog.info(line));
    const logStderrLine = makeLineSplitter((line) => sidecarLog.error(line));

    const reject = (error: Error) => {
      if (resolved || rejected) {
        return;
      }
      rejected = true;
      rejectStart(error);
    };

    const onStdout = (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      logStdoutLine(text);
      stdoutControlBuffer += text;
      const rawLines = stdoutControlBuffer.split(NEWLINE_SPLIT);
      stdoutControlBuffer = rawLines.pop() ?? "";
      const readyLine = rawLines
        .map((line) => line.trim())
        .find((line) => line.startsWith(`${READY_SENTINEL}:`));
      if (!readyLine || resolved) {
        return;
      }
      if (!child.pid) {
        reject(new Error("Sidecar became ready before a child pid was known."));
        return;
      }
      resolved = true;
      const readyPort = Number.parseInt(
        readyLine.slice(`${READY_SENTINEL}:`.length),
        DECIMAL
      );
      resolveStart({
        baseUrl: `http://${HOST}:${readyPort}`,
        hostname: HOST,
        port: readyPort,
        child,
      });
    };

    const onStderr = (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      stderrBuffer = (stderrBuffer + text).slice(-STDERR_TAIL_LIMIT);
      logStderrLine(text);
    };

    const onExit = (code: number | null, signal: NodeJS.Signals | null) => {
      if (resolved) {
        if (expectedExits.has(child)) {
          sidecarLog.info(`exited (code=${code} signal=${signal})`);
          return;
        }
        sidecarLog.error(
          `Sidecar exited unexpectedly (code=${code} signal=${signal})`
        );
        unexpectedExitListener?.();
        return;
      }
      if (rejected) {
        return;
      }
      if (PORT_IN_USE_PATTERN.test(stderrBuffer)) {
        reject(new SidecarPortInUseError(port));
        return;
      }
      reject(
        new Error(
          `Sidecar exited before ready (code=${code} signal=${signal}). Stderr:\n${stderrBuffer}`
        )
      );
    };

    child.on("error", (error) => reject(error as Error));
    child.stdout?.on("data", onStdout);
    child.stderr?.on("data", onStderr);
    child.once("exit", onExit);
  });
};

/** Probe `GET /health`, expecting a 200 with body "ok". Retries a few times. */
export const isSidecarReachable = async (origin: string): Promise<boolean> => {
  for (let attempt = 0; attempt < HEALTH_ATTEMPTS; attempt += 1) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), HEALTH_TIMEOUT_MS);
    try {
      const response = await fetch(new URL("/health", origin), {
        signal: controller.signal,
        redirect: "manual",
      });
      const body = await response.text();
      if (response.ok && body.trim() === "ok") {
        return true;
      }
    } catch {
      // Fetch rejects when the server is down — fall through to the retry.
    } finally {
      clearTimeout(timer);
    }
    if (attempt < HEALTH_ATTEMPTS - 1) {
      await delay(HEALTH_RETRY_DELAY_MS);
    }
  }
  return false;
};

/** Synchronously flag + SIGTERM the child. Safe to call from a quit handler. */
export const killSidecarSync = (child: ChildProcess): void => {
  expectedExits.add(child);
  if (child.exitCode === null && !child.killed) {
    child.kill("SIGTERM");
  }
};

/** Stop the sidecar: SIGTERM, then SIGKILL if it hasn't exited within 5s. */
export const stopSidecar = (child: ChildProcess): Promise<void> => {
  expectedExits.add(child);
  if (child.exitCode !== null || child.killed) {
    return Promise.resolve();
  }
  return new Promise<void>((resolveStop) => {
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      resolveStop();
    }, STOP_GRACE_MS);
    child.once("exit", () => {
      clearTimeout(timeout);
      resolveStop();
    });
    child.kill("SIGTERM");
  });
};
