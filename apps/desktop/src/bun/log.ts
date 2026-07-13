/**
 * Minimal file + console logger for the Bun main process.
 *
 * Replaces electron-log: writes human-readable lines to `<userLogs>/main.log` and
 * echoes them to the terminal (visible when the app is launched from a shell).
 * The log file is what a user can actually send us after a crash. All file I/O is
 * best-effort — logging must never throw and take the app down with it.
 */
import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { Utils } from "electrobun/bun";

type Level = "info" | "warn" | "error";

let logFile: string | null = null;

/** Resolve (and memoize) the log file path, creating its directory once. */
const resolveLogFile = (): string | null => {
  if (logFile) {
    return logFile;
  }
  try {
    const dir = Utils.paths.userLogs;
    mkdirSync(dir, { recursive: true });
    logFile = join(dir, "main.log");
    return logFile;
  } catch {
    return null;
  }
};

/** On-disk path of the main log file, or null if it could not be resolved. */
export const logFilePath = (): string | null => resolveLogFile();

const format = (part: unknown): string => {
  if (part instanceof Error) {
    return part.stack ?? part.message;
  }
  if (typeof part === "string") {
    return part;
  }
  try {
    return JSON.stringify(part);
  } catch {
    return String(part);
  }
};

const write = (level: Level, scope: string, parts: readonly unknown[]) => {
  const line = `[${level}]${scope ? ` [${scope}]` : ""} ${parts.map(format).join(" ")}`;
  const sink = level === "error" ? console.error : console.log;
  sink(line);
  const file = resolveLogFile();
  if (!file) {
    return;
  }
  try {
    appendFileSync(file, `${line}\n`);
  } catch {
    // Best-effort: a failed disk write must not surface to callers.
  }
};

/** Create a scoped logger; the scope is prefixed to every line it writes. */
export const scoped = (scope: string) => ({
  info: (...parts: unknown[]) => write("info", scope, parts),
  warn: (...parts: unknown[]) => write("warn", scope, parts),
  error: (...parts: unknown[]) => write("error", scope, parts),
});

export const log = scoped("");
