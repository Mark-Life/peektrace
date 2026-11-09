/**
 * Session cost from priced turns. Pure: every figure is a list-price estimate
 * unless the agent recorded its own (OpenCode), which then wins.
 */
import { type CostTokens, turnCost } from "../pricing";
import type { ParsedSession, SessionCost, Turn, TurnsCost } from "./schema";
import { opt } from "./usage";

/** What a turn needs to be priced. */
export type CostTurn = CostTokens & Pick<Turn, "model" | "costUsd">;

/** Set `costUsd` on each turn, keeping a cost the parser already read. */
export const priceTurns = <T extends CostTurn>(turns: readonly T[]) =>
  turns.map((t) =>
    t.costUsd === undefined ? { ...t, ...opt("costUsd", turnCost(t)) } : t
  );

/** Sum of already-priced turns; a turn with no `costUsd` names its model. */
const sumPriced = (
  turns: readonly Pick<Turn, "model" | "costUsd">[]
): TurnsCost => {
  let usd: number | undefined;
  const unpriced = new Set<string>();
  for (const t of turns) {
    if (t.costUsd === undefined) {
      unpriced.add(t.model);
    } else {
      usd = (usd ?? 0) + t.costUsd;
    }
  }
  return {
    ...opt("costUsd", usd),
    ...opt(
      "unpricedModels",
      unpriced.size > 0 ? [...unpriced].sort() : undefined
    ),
  };
};

/** Cost fields of a turn list: a list header, a subagent, inline sidechains. */
export const turnsCost = (turns: readonly CostTurn[]) =>
  sumPriced(priceTurns(turns));

/**
 * A session's cost from its priced turns: the agent's own figure when it
 * recorded one above 0, else the sum of priced turns; plus inline sidechain
 * turns and every subagent. Absent when there is nothing to price or report.
 */
export const sessionCost = (
  p: Pick<ParsedSession, "nativeCostUsd" | "subagents" | "sidechainCost">,
  pricedTurns: readonly Pick<Turn, "model" | "costUsd">[]
): SessionCost | undefined => {
  const own = sumPriced(pricedTurns);
  const native = p.nativeCostUsd;
  const main = native ?? own.costUsd;
  const parts = [p.sidechainCost, ...p.subagents].filter(
    (c): c is TurnsCost => c !== undefined
  );
  const subagentsUsd = parts.reduce((n, c) => n + (c.costUsd ?? 0), 0);
  const unpriced = new Set(native === undefined ? own.unpricedModels : []);
  for (const c of parts) {
    for (const m of c.unpricedModels ?? []) {
      unpriced.add(m);
    }
  }
  const unreadable = p.subagents.filter((s) => s.unreadable).length;
  const total =
    main === undefined && !parts.some((c) => c.costUsd !== undefined)
      ? undefined
      : (main ?? 0) + subagentsUsd;
  if (total === undefined && unpriced.size === 0 && unreadable === 0) {
    return;
  }
  return {
    source: native === undefined ? "estimate" : "agent",
    ...opt("totalUsd", total),
    ...opt("subagentsUsd", subagentsUsd > 0 ? subagentsUsd : undefined),
    ...opt(
      "unpricedModels",
      unpriced.size > 0 ? [...unpriced].sort() : undefined
    ),
    ...opt("unreadableSubagents", unreadable > 0 ? unreadable : undefined),
  };
};

const CENT = 0.005;

/** An estimated USD figure: `~$1.23`, or `<$0.01` below half a cent. */
export const fmtUsd = (n: number) =>
  n > 0 && n < CENT ? "<$0.01" : `~$${n.toFixed(2)}`;

/** Notes that qualify a session cost: subagents share, unpriced models, unreadable subagents. */
const costNotes = (cost: SessionCost) => [
  ...(cost.subagentsUsd ? [`subagents ${fmtUsd(cost.subagentsUsd)}`] : []),
  ...(cost.unpricedModels?.length
    ? [`unpriced: ${cost.unpricedModels.join(", ")}`]
    : []),
  ...(cost.unreadableSubagents
    ? [`${cost.unreadableSubagents} subagent(s) unreadable`]
    : []),
];

/**
 * A session cost as display text: the total (`—` when nothing is priced) and the
 * notes that qualify it, led by who reported it when the agent did.
 */
export const costSummary = (cost: SessionCost, agentLabel: string) => ({
  total: cost.totalUsd === undefined ? "—" : fmtUsd(cost.totalUsd),
  notes: [
    ...(cost.source === "agent" ? [`reported by ${agentLabel}`] : []),
    ...costNotes(cost),
  ],
});
