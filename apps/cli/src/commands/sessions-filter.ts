/** Client-side session selection: `sessions ls` filtering (`--since`, `--cwd`,
 * sort, `--limit`) and target resolution for single-session commands. */
import { homedir } from "node:os";
import { resolve } from "node:path";
import { byUpdatedDesc, timeOf } from "@workspace/core/services/sessions/order";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { Effect } from "effect";
import type { PeektraceClient } from "../client";
import { CliUserError } from "../errors";

const RELATIVE = /^(\d+)([hdw])$/;
const ISO_DATE = /^\d{4}-\d{2}-\d{2}(?:[T ][\d:.]+(?:Z|[+-]\d{2}:?\d{2})?)?$/;
const UNIT_MS = { h: 3_600_000, d: 86_400_000, w: 604_800_000 } as const;
const MAX_DATE_MS = 8.64e15;
const HAS_ZONE = /(?:Z|[+-]\d{2}:?\d{2})$/;

/** True when the leading `YYYY-MM-DD` is a real calendar day. */
const isRealDay = (text: string) => {
  const day = text.slice(0, 10);
  const ms = Date.parse(`${day}T00:00:00Z`);
  return Number.isFinite(ms) && new Date(ms).toISOString().slice(0, 10) === day;
};

/** ISO input as epoch ms. A time with no offset is UTC. NaN if invalid. */
const parseIso = (text: string) => {
  if (!(ISO_DATE.test(text) && isRealDay(text))) {
    return Number.NaN;
  }
  const iso = text.replace(" ", "T");
  return Date.parse(iso.includes("T") && !HAS_ZONE.test(iso) ? `${iso}Z` : iso);
};

/** A relative (`7d`, `12h`, `2w`) or ISO date to epoch ms; `undefined` if invalid. */
export const parseSince = (text: string, now: number) => {
  const trimmed = text.trim();
  const match = RELATIVE.exec(trimmed);
  const parsed = match
    ? now - Number(match[1]) * UNIT_MS[match[2] as keyof typeof UNIT_MS]
    : parseIso(trimmed);
  return Number.isFinite(parsed) && Math.abs(parsed) <= MAX_DATE_MS
    ? parsed
    : undefined;
};

const TRAILING_SEPARATORS = /[\\/]+$/;
const WINDOWS_PATH = /^(?:[a-z]:|\\\\)/i;

const stripSeparators = (path: string) =>
  path.replace(TRAILING_SEPARATORS, "") || "/";

/** Resolve a `--cwd` argument against this machine's working directory.
 * A leading `~` expands to the home dir, for callers that bypass the shell. */
export const resolveCwd = (text: string) =>
  stripSeparators(
    resolve(
      text === "~" || text.startsWith("~/") ? homedir() + text.slice(1) : text
    )
  );

/** True when the header's cwd is `cwd` or inside it. Rows without a cwd never match.
 * Both `/` and `\` separate segments; Windows paths compare case-insensitively. */
export const matchesCwd = (header: Pick<SessionHeader, "cwd">, cwd: string) => {
  if (header.cwd === undefined) {
    return false;
  }
  const fold = WINDOWS_PATH.test(cwd)
    ? (p: string) => p.toLowerCase().replaceAll("/", "\\")
    : (p: string) => p;
  const have = fold(stripSeparators(header.cwd));
  const want = fold(stripSeparators(cwd));
  if (want === "/" || have === want) {
    return true;
  }
  const next = have[want.length];
  return have.startsWith(want) && (next === "/" || next === "\\");
};

export interface SelectOptions {
  readonly cwd?: string | undefined;
  /** Max rows; `0` means no limit. */
  readonly limit: number;
  /** Epoch ms lower bound on `updatedAt`. */
  readonly since?: number | undefined;
}

/** Apply `--since`, `--cwd`, sort, then `--limit`. `hidden` counts rows cut by the limit. */
export const selectSessions = <
  H extends Pick<SessionHeader, "cwd" | "id" | "updatedAt">,
>(
  headers: readonly H[],
  { since, cwd, limit }: SelectOptions
) => {
  const kept = headers
    .filter((h) => {
      if (since === undefined) {
        return true;
      }
      const t = timeOf(h);
      return t !== undefined && t >= since;
    })
    .filter((h) => cwd === undefined || matchesCwd(h, cwd))
    .sort(byUpdatedDesc);
  const rows = limit === 0 ? kept : kept.slice(0, limit);
  return { rows, hidden: kept.length - rows.length };
};

/** The messages for invalid target combinations, worded for the caller's inputs. */
export interface TargetMessages {
  readonly idAndNewest: string;
  readonly noTarget: string;
  readonly parentWithoutId: string;
}

const CLI_TARGET_MESSAGES: TargetMessages = {
  parentWithoutId: "--parent needs an explicit subagent id.",
  idAndNewest: "Pass either a session id or --latest/--cwd, not both.",
  noTarget: "Pass a session id, --latest, or --cwd <dir>.",
};

export interface TargetRequest {
  /** Absolute `--cwd` directory, already resolved. */
  readonly cwd?: string | undefined;
  readonly id?: string | undefined;
  readonly latest: boolean;
  /** Defaults to messages that name the CLI flags. */
  readonly messages?: TargetMessages | undefined;
  /** `--parent` id; only valid together with an explicit subagent `id`. */
  readonly parent?: string | undefined;
}

export type Target =
  | { readonly _tag: "Picked"; readonly id: string }
  | { readonly _tag: "Invalid"; readonly message: string };

export type TargetPlan =
  | Target
  | { readonly _tag: "Newest"; readonly cwd: string | undefined };

/** Validate the request. `Newest` means the caller must list sessions and
 * call `newestTarget`; every other outcome is final. */
export const planTarget = ({
  id,
  latest,
  cwd,
  parent,
  messages = CLI_TARGET_MESSAGES,
}: TargetRequest): TargetPlan => {
  const wantsNewest = latest || cwd !== undefined;
  if (id?.startsWith("-")) {
    return { _tag: "Invalid", message: `Missing value for ${id}.` };
  }
  if (parent !== undefined && id === undefined) {
    return { _tag: "Invalid", message: messages.parentWithoutId };
  }
  if (id !== undefined && wantsNewest) {
    return { _tag: "Invalid", message: messages.idAndNewest };
  }
  if (id !== undefined) {
    return { _tag: "Picked", id };
  }
  if (!wantsNewest) {
    return { _tag: "Invalid", message: messages.noTarget };
  }
  return { _tag: "Newest", cwd };
};

/** The newest header (optionally under `cwd`), or an `Invalid` with a message. */
const newestTarget = (
  cwd: string | undefined,
  headers: readonly Pick<SessionHeader, "cwd" | "id" | "updatedAt">[]
): Target => {
  const [newest] = selectSessions(headers, { limit: 1, cwd }).rows;
  return newest
    ? { _tag: "Picked", id: newest.id }
    : {
        _tag: "Invalid",
        message: `No sessions found${cwd === undefined ? "" : ` under ${cwd}`}.`,
      };
};

/** Pick the session `analyze` runs on: an explicit id, or the newest header
 * (optionally under `--cwd`). Invalid combinations and empty matches carry a message. */
export const pickTarget = (
  request: TargetRequest,
  headers: readonly Pick<SessionHeader, "cwd" | "id" | "updatedAt">[]
): Target => {
  const plan = planTarget(request);
  return plan._tag === "Newest" ? newestTarget(plan.cwd, headers) : plan;
};

/** The session id a command runs on. Lists sessions over RPC only when the
 * request asks for the newest one. Fails with a `CliUserError` on bad input. */
export const resolveTargetId = (
  client: PeektraceClient,
  request: TargetRequest
) =>
  Effect.gen(function* () {
    const plan = planTarget(request);
    const target =
      plan._tag === "Newest"
        ? newestTarget(plan.cwd, yield* client.sessions.list({}))
        : plan;
    if (target._tag === "Invalid") {
      return yield* new CliUserError({ message: target.message });
    }
    return target.id;
  });
