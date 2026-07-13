/**
 * Window frame persistence — the Electrobun replacement for electron-window-state.
 *
 * Reads/writes `<userData>/window-state.json`. All I/O is best-effort: a missing
 * or corrupt file falls back to the centered default, and write failures are
 * swallowed so a locked disk can't break shutdown.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Utils } from "electrobun/bun";

export interface WindowFrame {
  readonly height: number;
  readonly width: number;
  readonly x: number;
  readonly y: number;
}

const DEFAULT_WIDTH = 1280;
const DEFAULT_HEIGHT = 800;
const MIN_WIDTH = 768;
const MIN_HEIGHT = 480;

const defaultFrame = (): WindowFrame => ({
  x: 120,
  y: 120,
  width: DEFAULT_WIDTH,
  height: DEFAULT_HEIGHT,
});

const stateFile = (): string | null => {
  try {
    const dir = Utils.paths.userData;
    mkdirSync(dir, { recursive: true });
    return join(dir, "window-state.json");
  } catch {
    return null;
  }
};

const isFrame = (value: unknown): value is WindowFrame => {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const frame = value as Record<string, unknown>;
  return (
    typeof frame.x === "number" &&
    typeof frame.y === "number" &&
    typeof frame.width === "number" &&
    typeof frame.height === "number"
  );
};

/** Load the persisted frame, clamped to sane minimums, or the default. */
export const loadWindowState = (): WindowFrame => {
  const file = stateFile();
  if (!file) {
    return defaultFrame();
  }
  try {
    const parsed: unknown = JSON.parse(readFileSync(file, "utf8"));
    if (!isFrame(parsed)) {
      return defaultFrame();
    }
    return {
      x: parsed.x,
      y: parsed.y,
      width: Math.max(MIN_WIDTH, parsed.width),
      height: Math.max(MIN_HEIGHT, parsed.height),
    };
  } catch {
    return defaultFrame();
  }
};

/** Persist the current frame. No-op on any failure. */
export const saveWindowState = (frame: WindowFrame): void => {
  const file = stateFile();
  if (!file) {
    return;
  }
  try {
    writeFileSync(file, JSON.stringify(frame));
  } catch {
    // Best-effort persistence.
  }
};
