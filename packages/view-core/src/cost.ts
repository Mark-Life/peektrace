/** A session cost as display text. The figures come from the server. */
import type {
  AgentId,
  Provider,
  SessionCost,
  SessionHeader,
} from "./api-types.gen";

const CENT = 0.005;
const USD_DECIMALS = 2;
const PERCENT = 100;

/** `SessionCost` where every field but `source` may be unset, as on a list row. */
type CostFields = Pick<SessionCost, "source"> & {
  readonly [K in Exclude<keyof SessionCost, "source">]?:
    | SessionCost[K]
    | undefined;
};

const usd = (n: number, prefix: string) =>
  n > 0 && n < CENT ? "<$0.01" : `${prefix}$${n.toFixed(USD_DECIMALS)}`;

/** An estimated USD figure: `~$1.23`, or `<$0.01` below half a cent. */
export const fmtUsd = (n: number) => usd(n, "~");

/** A figure the agent recorded: `$1.23`, or `<$0.01` below half a cent. */
export const fmtRecordedUsd = (n: number) => usd(n, "");

/** `estimate from tokens ~$34.37 (93% lower)`. */
export const estimateWarning = ({
  estimate,
  recorded,
}: {
  readonly estimate: number;
  readonly recorded: number;
}) => {
  const gap = recorded > 0 ? (estimate - recorded) / recorded : 0;

  return `estimate from tokens ${fmtUsd(estimate)} (${Math.abs(gap * PERCENT).toFixed(0)}% ${gap < 0 ? "lower" : "higher"})`;
};

/** Display label per transcript provider. */
const PROVIDER_LABEL: Readonly<Record<Provider, string>> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
  pi: "Pi",
};

/**
 * The agent's display name; a newer server's unknown provider shows its raw
 * id.
 */
export const providerLabel = (provider: Provider) =>
  // SAFETY: the record covers the union; the lookup can still miss at runtime.
  // oxlint-disable-next-line typescript/no-unnecessary-condition -- see above
  PROVIDER_LABEL[provider] ?? provider;

/**
 * The quiet note for the estimate beside a full record. Only Claude Code bills
 * calls its transcript does not log, so only its note says so.
 */
const estimateNote = (estimate: number, provider: Provider) =>
  provider === "claude-code"
    ? `Estimate from transcript: ${fmtUsd(estimate)}. Transcripts miss some billed calls.`
    : `Estimate at list price: ${fmtUsd(estimate)}.`;

/**
 * The warning or the quiet note for the estimate beside a full record. Core
 * decides when it warns (`estimateWarning`).
 */
const estimateLines = (cost: CostFields, provider: Provider) => {
  const { estimateUsd: estimate, recordedUsd: recorded } = cost;

  if (
    estimate === undefined ||
    recorded === undefined ||
    cost.source !== "agent" ||
    cost.recordPartial === true
  ) {
    return { estimateNote: undefined, warning: undefined };
  }

  return cost.estimateWarning === true
    ? {
        estimateNote: undefined,
        warning: estimateWarning({ estimate, recorded }),
      }
    : { estimateNote: estimateNote(estimate, provider), warning: undefined };
};

/** Notes that qualify a session cost after its source. */
const costNotes = (cost: CostFields, agentLabel: string) => [
  ...(cost.subagentsUsd !== undefined && cost.subagentsUsd !== 0
    ? [`incl. subagents ${fmtUsd(cost.subagentsUsd)}`]
    : []),
  ...(cost.recordPartial === true && cost.recordedUsd !== undefined
    ? [
        `${agentLabel} recorded ${fmtRecordedUsd(cost.recordedUsd)} for part of the session`,
      ]
    : []),
  ...(cost.unpricedModels !== undefined && cost.unpricedModels.length > 0
    ? [`unpriced: ${cost.unpricedModels.join(", ")}`]
    : []),
  ...(cost.unreadableSubagents !== undefined && cost.unreadableSubagents !== 0
    ? [`${cost.unreadableSubagents} subagent(s) unreadable`]
    : []),
];

const fmtTotal = (cost: CostFields) => {
  if (cost.totalUsd === undefined) {
    return "—";
  }

  return cost.source === "agent"
    ? fmtRecordedUsd(cost.totalUsd)
    : fmtUsd(cost.totalUsd);
};

/**
 * A session cost as display text: the total (`—` when nothing is priced),
 * the notes that qualify it led by its source, and beside a full record
 * either a warning (a likely price problem) or a quiet estimate note.
 */
export const costSummary = (cost: CostFields, provider: Provider) => ({
  notes: [
    cost.source === "agent"
      ? `recorded by ${providerLabel(provider)}`
      : "estimated from tokens",
    ...costNotes(cost, providerLabel(provider)),
  ],
  total: fmtTotal(cost),
  ...estimateLines(cost, provider),
});

/** The provider id of a list row's agent. */
const PROVIDER_OF: Readonly<Record<AgentId, Provider>> = {
  claude: "claude-code",
  codex: "codex",
  opencode: "opencode",
  pi: "pi",
};

/**
 * A list row's cost, marked as the session detail marks it: `$1.23` when the
 * agent recorded it, `~$1.23` when estimated from tokens, plus `*` when some
 * models are unpriced (`—*` when none is). `label` is a short version for
 * screen readers; `title` holds the source and notes. `undefined` when the row
 * has no cost.
 */
export const headerCost = (
  h: Pick<
    SessionHeader,
    "agent" | "costSource" | "costUsd" | "subagentsUsd" | "unpricedModels"
  >
) => {
  const { subagentsUsd, unpricedModels } = h;

  // Core leaves a non-finite cost out; guard the row against one anyway.
  const costUsd = Number.isFinite(h.costUsd) ? h.costUsd : undefined;

  const unpriced = unpricedModels !== undefined && unpricedModels.length > 0;
  const recorded = h.costSource === "agent";

  const summary =
    costUsd === undefined && !unpriced
      ? undefined
      : costSummary(
          {
            source: recorded ? "agent" : "estimate",
            subagentsUsd,
            totalUsd: costUsd,
            unpricedModels,
          },
          PROVIDER_OF[h.agent]
        );

  const unpricedNote = unpriced ? ", some models unpriced" : "";
  const amount = costUsd === undefined ? "" : fmtRecordedUsd(costUsd);

  const estimated =
    costUsd === undefined
      ? `cost unknown${unpricedNote}`
      : `cost about ${amount}, estimated${unpricedNote}`;

  const label = recorded ? `cost ${amount}, recorded` : estimated;

  return summary === undefined
    ? undefined
    : {
        label,
        text: unpriced ? `${summary.total}*` : summary.total,
        title: `Cost: ${[summary.total, ...summary.notes].join(" · ")}`,
      };
};
