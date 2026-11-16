/** The app that started a session (e.g. t3code), as a badge tooltip. */
import type { Launcher } from "./api-types.gen";

/** Who started the session, plus the branch and worktree it gave it. */
export const launcherHint = (l: Launcher) =>
  [
    `Started from ${l.name}`,
    ...(l.branch === undefined ? [] : [`branch ${l.branch}`]),
    ...(l.worktreePath === undefined ? [] : [`worktree ${l.worktreePath}`]),
  ].join(" · ");
