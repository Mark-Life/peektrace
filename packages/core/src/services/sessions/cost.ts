/**
 * Session cost from priced turns. Pure: every figure is a list-price estimate
 * unless the agent recorded its own (OpenCode), which then wins.
 */
import { type CostTokens, turnCost } from "../pricing";
import type { ParsedSession, SessionCost, SubagentRef, Turn } from "./schema";
import { opt } from "./usage";

/** What a turn needs to be priced. */
export type CostTurn = CostTokens & Pick<Turn, "model" | "costUsd">;

/** Sum of the priced turns, and the models that had no rate card. */
const foldTurns = (turns: readonly CostTurn[]) => {
  let usd: number | undefined;
  const unpriced = new Set<string>();
  for (const t of turns) {
    const cost = t.costUsd ?? turnCost(t);
    if (cost === undefined) {
      unpriced.add(t.model);
    } else {
      usd = (usd ?? 0) + cost;
    }
  }
  return { usd, unpriced: [...unpriced].sort() };
};

const nonEmpty = <T>(xs: readonly T[]) => (xs.length > 0 ? xs : undefined);

/** Set `costUsd` on each turn, keeping a cost the parser already read. */
export const priceTurns = <T extends CostTurn>(turns: readonly T[]) =>
  turns.map((t) =>
    t.costUsd === undefined ? { ...t, ...opt("costUsd", turnCost(t)) } : t
  );

/** The cost fields of one subagent, from its parsed turns. */
export const subagentCost = (
  turns: readonly CostTurn[]
): Pick<SubagentRef, "costUsd" | "unpricedModels"> => {
  const { usd, unpriced } = foldTurns(turns);
  return {
    ...opt("costUsd", usd),
    ...opt("unpricedModels", nonEmpty(unpriced)),
  };
};

/** The cost fields of a list header, from the main transcript's turns. */
export const headerCost = (turns: readonly CostTurn[]) => {
  const { usd, unpriced } = foldTurns(turns);
  return {
    ...opt("costUsd", usd),
    ...opt("costUnpriced", unpriced.length > 0 ? (true as const) : undefined),
  };
};

/**
 * A session's cost: the agent's own figure when it recorded one above 0, else
 * the sum of priced turns; plus every subagent's cost. Absent when there is
 * nothing to price or report.
 */
export const sessionCost = (
  p: Pick<ParsedSession, "nativeCostUsd" | "subagents">,
  turns: readonly CostTurn[]
): SessionCost | undefined => {
  const own = foldTurns(turns);
  const native = p.nativeCostUsd;
  const main = native ?? own.usd;
  const subagents = p.subagents.filter((s) => s.costUsd !== undefined);
  const subagentsUsd = subagents.reduce((n, s) => n + (s.costUsd ?? 0), 0);
  const unpriced = new Set(native === undefined ? own.unpriced : []);
  for (const s of p.subagents) {
    for (const m of s.unpricedModels ?? []) {
      unpriced.add(m);
    }
  }
  const total =
    main === undefined && subagents.length === 0
      ? undefined
      : (main ?? 0) + subagentsUsd;
  if (total === undefined && unpriced.size === 0) {
    return;
  }
  return {
    source: native === undefined ? "estimate" : "agent",
    ...opt("totalUsd", total),
    ...opt("subagentsUsd", subagentsUsd > 0 ? subagentsUsd : undefined),
    ...opt("unpricedModels", nonEmpty([...unpriced].sort())),
  };
};
