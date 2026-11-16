/** A session cost as display text. The figures come from the server. */
import type { SessionCost } from "./api-types.gen";

const CENT = 0.005;
const USD_DECIMALS = 2;

/** An estimated USD figure: `~$1.23`, or `<$0.01` below half a cent. */
export const fmtUsd = (n: number) =>
  n > 0 && n < CENT ? "<$0.01" : `~$${n.toFixed(USD_DECIMALS)}`;

/** Notes that qualify a session cost: subagents share, unpriced models, unreadable subagents. */
const costNotes = (cost: SessionCost) => [
  ...(cost.subagentsUsd !== undefined && cost.subagentsUsd !== 0
    ? [`subagents ${fmtUsd(cost.subagentsUsd)}`]
    : []),
  ...(cost.unpricedModels !== undefined && cost.unpricedModels.length > 0
    ? [`unpriced: ${cost.unpricedModels.join(", ")}`]
    : []),
  ...(cost.unreadableSubagents !== undefined && cost.unreadableSubagents !== 0
    ? [`${cost.unreadableSubagents} subagent(s) unreadable`]
    : []),
];

/**
 * A session cost as display text: the total (`—` when nothing is priced) and the
 * notes that qualify it, led by who reported it when the agent did.
 */
export const costSummary = (cost: SessionCost, agentLabel: string) => ({
  notes: [
    ...(cost.source === "agent" ? [`reported by ${agentLabel}`] : []),
    ...costNotes(cost),
  ],
  total: cost.totalUsd === undefined ? "—" : fmtUsd(cost.totalUsd),
});
