/** Names, monograms and tooltips for the session marks: the app that started a
 * session, the harness that ran it, the maker of its model, and its worktree. */
import type {
  Launcher,
  ModelProvider,
  Provider,
  SessionHeader,
  Worktree,
} from "./api-types.gen";
import { providerLabel } from "./cost";

/** One small mark: `text` is drawn, `label` is the accessible name, `title`
 * the tooltip. */
export interface Mark {
  readonly label: string;
  readonly text: string;
  readonly title: string;
}

/** Two-letter monogram per harness. */
const HARNESS_TEXT: Readonly<Record<Provider, string>> = {
  "claude-code": "CC",
  codex: "Cx",
  opencode: "OC",
  pi: "Pi",
};

/** Monograms of known launchers, by lowercase name. */
const LAUNCHER_TEXT: ReadonlyMap<string, string> = new Map([["t3code", "T3"]]);

const MONOGRAM_LENGTH = 2;

/** First two letters of `name`, first one upper case; `?` when empty. */
const monogramOf = (name: string) => {
  const letters = name
    .replaceAll(/[^\p{L}\p{N}]/gu, "")
    .slice(0, MONOGRAM_LENGTH);

  return letters === ""
    ? "?"
    : `${letters.charAt(0).toUpperCase()}${letters.slice(1)}`;
};

/** The harness that ran the session. */
export const harnessMark = (provider: Provider): Mark => {
  const name = providerLabel(provider);

  return {
    label: name,
    // SAFETY: the record covers the union; a newer server may send another id.
    // oxlint-disable-next-line typescript/no-unnecessary-condition -- see above
    text: HARNESS_TEXT[provider] ?? monogramOf(name),
    title: name,
  };
};

/** Who started the session, plus the branch and worktree it gave it. */
const launcherHint = (l: Launcher) =>
  [
    `Started from ${l.name}`,
    ...(l.branch === undefined ? [] : [`branch ${l.branch}`]),
    ...(l.worktreePath === undefined ? [] : [`worktree ${l.worktreePath}`]),
  ].join(" · ");

/** The app that started the session. */
export const launcherMark = (l: Launcher): Mark => ({
  label: `Started from ${l.name}`,
  text: LAUNCHER_TEXT.get(l.name.toLowerCase()) ?? monogramOf(l.name),
  title: launcherHint(l),
});

/** `Anthropic · claude-opus-5-5`; an unmapped model names no maker. */
const providerLine = (provider: ModelProvider | undefined, model: string) =>
  `${provider?.name ?? "Unknown provider"} · ${model}`;

/** The models a session used, main model first, each once. */
export const sessionModels = ({
  model,
  models = [],
}: {
  readonly model?: string;
  readonly models?: readonly string[];
}) =>
  [...new Set([...(model === undefined ? [] : [model]), ...models])].filter(
    (m) => m !== ""
  );

/** Tooltip for the model maker mark: the main model's line, then every other
 * model one per line. The wire names only the main model's maker. */
const providerTooltip = ({
  provider,
  models,
}: {
  readonly provider: ModelProvider | undefined;
  readonly models: readonly string[];
}) => {
  const [main, ...rest] = models;

  return main === undefined
    ? ""
    : [providerLine(provider, main), ...rest].join("\n");
};

/** The maker of the session's main model; `undefined` when no model is known. */
export const providerMark = ({
  provider,
  models,
}: {
  readonly provider: ModelProvider | undefined;
  readonly models: readonly string[];
}): Mark | undefined => {
  const [main] = models;

  return main === undefined
    ? undefined
    : {
        label: providerLine(provider, main),
        text: provider === undefined ? "?" : monogramOf(provider.name),
        title: providerTooltip({ models, provider }),
      };
};

/** The label before the title of the thread that started a session. */
const STARTED_BY = "Started by";

/** `Started by: <title>`. */
export const startedBy = (title: string) => `${STARTED_BY}: ${title}`;

/** Tooltip for the worktree marker: its path, then who started the session. */
export const worktreeHint = ({
  worktree,
  parentTitle,
}: {
  readonly worktree: Worktree;
  readonly parentTitle: string | undefined;
}) =>
  [
    `Worktree: ${worktree.path}`,
    ...(parentTitle === undefined || parentTitle === ""
      ? []
      : [startedBy(parentTitle)]),
  ].join("\n");

/** The branch worth showing: none for an empty or detached `HEAD`. */
export const visibleBranch = (branch: string | undefined) =>
  branch !== undefined && branch !== "" && branch !== "HEAD"
    ? branch
    : undefined;

/** A session's branch: the transcript's, else the worktree's when the
 * transcript has none or a detached `HEAD`. Never the worktree folder name. */
export const sessionBranch = ({
  gitBranch,
  worktree,
}: Pick<SessionHeader, "gitBranch" | "worktree">) =>
  visibleBranch(gitBranch) ?? visibleBranch(worktree?.branch);

/** A session's project name: its worktree's main checkout, else the last
 * folder of its cwd, else the agent's project slug. */
export const projectName = ({
  cwd,
  project,
  worktree,
}: Pick<SessionHeader, "cwd" | "project" | "worktree">) =>
  (worktree?.project === undefined || worktree.project === ""
    ? undefined
    : worktree.project) ??
  (cwd ?? "").split(/[\\/]/u).findLast((s) => s !== "") ??
  project;
