/** A session cost as display text. The figures come from the server. */
import type { SessionCost } from "./api-types.gen";

const CENT = 0.005;
const USD_DECIMALS = 2;
const PERCENT = 100;

const usd = (n: number, prefix: string) =>
  n > 0 && n < CENT ? "<$0.01" : `${prefix}$${n.toFixed(USD_DECIMALS)}`;

/** An estimated USD figure: `~$1.23`, or `<$0.01` below half a cent. */
export const fmtUsd = (n: number) => usd(n, "~");

/** A figure the agent recorded: `$1.23`, or `<$0.01` below half a cent. */
export const fmtRecordedUsd = (n: number) => usd(n, "");

/** `estimate from tokens ~$34.37 (93% lower)`. */
export const estimateWarning = ({
  estimate,
  gap,
}: {
  readonly estimate: number;
  readonly gap: number;
}) =>
  `estimate from tokens ${fmtUsd(estimate)} (${Math.abs(gap * PERCENT).toFixed(0)}% ${gap < 0 ? "lower" : "higher"})`;

/** Notes that qualify a session cost after its source. */
const costNotes = (cost: SessionCost, agentLabel: string) => [
  ...(cost.subagentsUsd !== undefined && cost.subagentsUsd !== 0
    ? [`subagents ${fmtUsd(cost.subagentsUsd)}`]
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

const fmtTotal = (cost: SessionCost) => {
  if (cost.totalUsd === undefined) {
    return "—";
  }

  return cost.source === "agent"
    ? fmtRecordedUsd(cost.totalUsd)
    : fmtUsd(cost.totalUsd);
};

/**
 * A session cost as display text: the total (`—` when nothing is priced),
 * the notes that qualify it led by its source, and a warning when the
 * agent's record and our estimate disagree.
 */
export const costSummary = (cost: SessionCost, agentLabel: string) => ({
  notes: [
    cost.source === "agent"
      ? `recorded by ${agentLabel}`
      : "estimated from tokens",
    ...costNotes(cost, agentLabel),
  ],
  total: fmtTotal(cost),
  warning:
    cost.estimateGap === undefined || cost.estimateUsd === undefined
      ? undefined
      : estimateWarning({ estimate: cost.estimateUsd, gap: cost.estimateGap }),
});
