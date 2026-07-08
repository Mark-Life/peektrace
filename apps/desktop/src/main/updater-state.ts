import type { DesktopUpdateStatus } from "../shared/update";

export type UpdateCheckTrigger = "boot" | "interval" | "manual";

export interface UpdateCheckPlan {
  readonly check: boolean;
  readonly promptVersionAfterCheck: string | null;
}

export interface CompletedUpdateCheckInput {
  readonly availableVersion: string | null;
  readonly stagedVersion: string | null;
  readonly trigger: UpdateCheckTrigger;
  readonly updateAvailable: boolean;
}

export interface DownloadedUpdateInput {
  readonly declinedVersion: string | null;
  readonly incomingVersion: string;
  readonly stagedVersion: string | null;
  readonly trigger: UpdateCheckTrigger;
}

export interface DownloadedUpdatePlan {
  readonly declinedVersion: string | null;
  readonly promptVersion: string | null;
  readonly stagedVersion: string;
  readonly status: DesktopUpdateStatus;
}

// Strips a pre-release/build suffix ("-beta", "+build") so only the numeric core
// is compared.
const VERSION_SUFFIX_SEPARATOR = /[+-]/;

/** Split a version into numeric parts, or null when any segment is unparseable. */
const parseVersionParts = (version: string): readonly number[] | null => {
  const core = version.trim().split(VERSION_SUFFIX_SEPARATOR, 1)[0];
  if (!core) {
    return null;
  }
  const parts = core.split(".").map((part) => Number.parseInt(part, 10));
  return parts.every((part) => Number.isInteger(part) && part >= 0)
    ? parts
    : null;
};

/**
 * Compare two versions: 1 when left is newer, -1 when older, 0 when equal, and
 * null when either side is unparseable. Missing trailing parts are treated as 0.
 */
export const compareUpdateVersions = (
  left: string,
  right: string
): number | null => {
  const leftParts = parseVersionParts(left);
  const rightParts = parseVersionParts(right);
  if (!(leftParts && rightParts)) {
    return null;
  }
  const length = Math.max(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const l = leftParts[index] ?? 0;
    const r = rightParts[index] ?? 0;
    if (l !== r) {
      return l > r ? 1 : -1;
    }
  }
  return 0;
};

/** True when `incoming` is strictly newer than `declined` (falls back to `!==` on unparseable). */
const isNewerUpdateVersion = (
  incomingVersion: string,
  declinedVersion: string
): boolean => {
  const comparison = compareUpdateVersions(incomingVersion, declinedVersion);
  return comparison === null
    ? incomingVersion !== declinedVersion
    : comparison > 0;
};

/** Decide whether to run a check and whether to re-prompt the staged version afterward. */
export const planUpdateCheck = (input: {
  readonly stagedVersion: string | null;
  readonly trigger: UpdateCheckTrigger;
}): UpdateCheckPlan => ({
  check: true,
  promptVersionAfterCheck:
    input.trigger === "manual" ? input.stagedVersion : null,
});

/**
 * After a completed check, decide which version (if any) to prompt. Only manual
 * checks with a staged version prompt: they re-prompt the staged version when no
 * strictly-newer version won, and defer to the download handler when one did.
 */
export const planCompletedUpdateCheck = (
  input: CompletedUpdateCheckInput
): { readonly promptVersion: string | null } => {
  if (input.trigger !== "manual" || !input.stagedVersion) {
    return { promptVersion: null };
  }
  if (!input.updateAvailable) {
    return { promptVersion: input.stagedVersion };
  }
  if (!input.availableVersion) {
    return { promptVersion: null };
  }
  const comparison = compareUpdateVersions(
    input.availableVersion,
    input.stagedVersion
  );
  if (comparison === null) {
    return {
      promptVersion:
        input.availableVersion === input.stagedVersion
          ? input.stagedVersion
          : null,
    };
  }
  return { promptVersion: comparison <= 0 ? input.stagedVersion : null };
};

/**
 * Stage a downloaded update and decide whether to prompt now. A declined version
 * is NOT re-prompted until a strictly-newer one arrives — this is the anti-nag core.
 */
export const planDownloadedUpdate = (
  input: DownloadedUpdateInput
): DownloadedUpdatePlan => {
  const prompt =
    input.trigger === "manual" ||
    !input.declinedVersion ||
    isNewerUpdateVersion(input.incomingVersion, input.declinedVersion);
  return {
    stagedVersion: input.incomingVersion,
    declinedVersion: input.declinedVersion,
    status: { state: "downloaded", version: input.incomingVersion },
    promptVersion: prompt ? input.incomingVersion : null,
  };
};

/** Move active update states (available/downloading/error) to error; leave the rest untouched. */
export const statusAfterUpdateError = (
  status: DesktopUpdateStatus,
  message: string
): DesktopUpdateStatus => {
  if (
    status.state === "available" ||
    status.state === "downloading" ||
    status.state === "error"
  ) {
    return { state: "error", version: status.version, message };
  }
  return status;
};
