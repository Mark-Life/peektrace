/**
 * Corpus-level panels: what filled the context window, what a fan-out re-paid,
 * what was fetched twice, and where the tokens went.
 *
 * Four panels, one pass, all keyed so a row deep-links back to its sessions.
 *
 * Context concentration ranks sessions by the largest single-request context
 * they reached, with the cache-read split beside it, so a session that refilled
 * the window is distinguishable from one that held it. The lever is in the
 * tail — the top 1% of sessions hold about a third of every cache-read token —
 * so the head of the distribution is stated as a caveat, which a capped table
 * cannot show.
 *
 * Fan-out prefix cost sums the first-turn cache creation of every agent in one
 * `wf_*` run: the prompt prefix each spawned agent re-pays before it does any
 * work. It reads `run` and `kind`, never a tool name, and names the kind it
 * filtered to.
 *
 * Duplicate web fetches count the same url or query answered more than once,
 * and mark the ones that repeated entirely inside one run, which is the case a
 * workflow author can fix.
 *
 * Cost rolls tokens up by session kind, by model and by project. Every dollar
 * figure is an ESTIMATE: no transcript records per-call tokens, the rates are
 * published list prices, and a model id with no rate card is priced at sonnet
 * rates and named in a caveat rather than hidden.
 */
import type { AccumulatorFactory, FinishContext } from "../accumulate";
import {
  compareKeyed,
  rank,
  round,
  share,
  sortBy,
  sortedRecord,
  sum,
  topKey,
  uniqSorted,
} from "../rank";
import type {
  ContextRow,
  CostRow,
  FanoutRow,
  SessionFact,
  StatsFact,
  WebRepeatRow,
} from "../schema";

/** USD per million tokens for one model family. */
export interface ModelPrice {
  readonly cacheRead: number;
  readonly cacheWrite: number;
  readonly input: number;
  readonly output: number;
}

/**
 * Published list prices per million tokens, checked 2026-08-10. No batch
 * discount, no long-context tier, no account terms. Cache reads and cache
 * writes bill separately from plain input.
 */
export const DEFAULT_PRICING: Readonly<Record<string, ModelPrice>> = {
  haiku: { input: 1, output: 5, cacheWrite: 1.25, cacheRead: 0.1 },
  opus: { input: 15, output: 75, cacheWrite: 18.75, cacheRead: 1.5 },
  sonnet: { input: 3, output: 15, cacheWrite: 3.75, cacheRead: 0.3 },
};

/** Family names tried as substrings of a model id. */
const FAMILIES = ["opus", "sonnet", "haiku"] as const;

/** Family used when a model id matches nothing. Such ids are named in a caveat. */
const FALLBACK_FAMILY = "sonnet";

/** Model recorded for a session whose transcript names none. */
const UNKNOWN_MODEL = "(unknown)";

/** Scales: the rate table is quoted per million, and a caveat shortens counts. */
const BILLION = 1e9;
const MILLION = 1e6;
const THOUSAND = 1e3;

/** A share times this reads as a percentage. */
const PERCENT = 100;

/** Share of sessions the context caveat calls out. */
const CONCENTRATION_HEAD = 0.01;

/** Tools whose primary argument is a url or a query. */
export const WEB_TOOLS: ReadonlySet<string> = new Set([
  "WebFetch",
  "WebSearch",
  "web_search",
  "web_search_call",
]);

/** The four corpus panels, produced in one pass. */
export interface CorpusPanels {
  readonly context: readonly ContextRow[];
  readonly cost: readonly CostRow[];
  readonly fanout: readonly FanoutRow[];
  readonly web: readonly WebRepeatRow[];
}

/** The token columns every cost row carries. Derived, never restated. */
type Tokens = Pick<
  CostRow,
  "cacheCreationTokens" | "cacheReadTokens" | "inputTokens" | "outputTokens"
>;

/** Which rate card a model id resolved to. */
interface PriceMatch {
  /** False when the id matched nothing and fell back to the sonnet family. */
  readonly priced: boolean;
  readonly rates: ModelPrice;
}

/**
 * Match a model id to its rates: exact table key first, then a family substring
 * (`opus`, `sonnet`, `haiku`), then the sonnet fallback with `priced: false` so
 * the caller can name the id as a guess.
 */
export const priceFor = (modelId: string): PriceMatch => {
  const id = modelId.trim().toLowerCase();
  const exact = DEFAULT_PRICING[id];
  if (exact) {
    return { rates: exact, priced: true };
  }
  for (const family of FAMILIES) {
    const rates = DEFAULT_PRICING[family];
    if (id.includes(family) && rates) {
      return { rates, priced: true };
    }
  }
  const fallback = DEFAULT_PRICING[FALLBACK_FAMILY];
  return {
    rates: fallback ?? { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 },
    priced: false,
  };
};

/** Estimated USD for one token block at one rate card. */
export const estimateCost = (tokens: Tokens, rates: ModelPrice): number =>
  (tokens.inputTokens * rates.input +
    tokens.outputTokens * rates.output +
    tokens.cacheReadTokens * rates.cacheRead +
    tokens.cacheCreationTokens * rates.cacheWrite) /
  MILLION;

const noTokens = (): Tokens => ({
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
});

const addTokens = (a: Tokens, b: Tokens): Tokens => ({
  inputTokens: a.inputTokens + b.inputTokens,
  outputTokens: a.outputTokens + b.outputTokens,
  cacheReadTokens: a.cacheReadTokens + b.cacheReadTokens,
  cacheCreationTokens: a.cacheCreationTokens + b.cacheCreationTokens,
});

const mapTokens = (t: Tokens, f: (n: number) => number): Tokens => ({
  inputTokens: f(t.inputTokens),
  outputTokens: f(t.outputTokens),
  cacheReadTokens: f(t.cacheReadTokens),
  cacheCreationTokens: f(t.cacheCreationTokens),
});

/** One transcript, held for as long as the pass runs. Bounded by session count. */
interface SessionAgg extends ContextRow {
  readonly firstTurnCacheCreation: number;
  readonly inputTokens: number;
  readonly models: readonly string[];
  readonly run: string | null;
}

/** Calls charged to one workflow run. */
interface RunCalls {
  calls: number;
  sec: number;
}

/** One url or query, and every call that asked for it. */
interface WebAgg {
  calls: number;
  chars: number;
  /** `run ?? ""`, so a repeat outside any run cannot read as same-run. */
  runs: Set<string>;
  sessions: Set<string>;
  tool: string;
}

/** Tokens rolled up under one cost key. */
interface CostAgg {
  sessions: number;
  tokens: Tokens;
}

const sessionAggOf = (fact: SessionFact): SessionAgg => ({
  key: fact.sid,
  sid: fact.sid,
  agent: fact.agent,
  project: fact.project,
  kind: fact.kind,
  run: fact.run,
  models: fact.models,
  maxContextTokens: fact.maxContextTokens,
  firstTurnCacheCreation: fact.firstTurnCacheCreation,
  inputTokens: fact.inputTokens,
  outputTokens: fact.outputTokens,
  cacheReadTokens: fact.cacheReadTokens,
  cacheCreationTokens: fact.cacheCreationTokens,
});

/** Fold a second transcript carrying an id already seen: peak wins, tokens sum. */
const mergeSession = (prev: SessionAgg, fact: SessionFact): SessionAgg => ({
  ...prev,
  run: prev.run ?? fact.run,
  models: uniqSorted([...prev.models, ...fact.models]),
  maxContextTokens: Math.max(prev.maxContextTokens, fact.maxContextTokens),
  firstTurnCacheCreation:
    prev.firstTurnCacheCreation + fact.firstTurnCacheCreation,
  inputTokens: prev.inputTokens + fact.inputTokens,
  outputTokens: prev.outputTokens + fact.outputTokens,
  cacheReadTokens: prev.cacheReadTokens + fact.cacheReadTokens,
  cacheCreationTokens: prev.cacheCreationTokens + fact.cacheCreationTokens,
});

const tokensOf = (s: SessionAgg): Tokens => ({
  inputTokens: s.inputTokens,
  outputTokens: s.outputTokens,
  cacheReadTokens: s.cacheReadTokens,
  cacheCreationTokens: s.cacheCreationTokens,
});

/** A session's tokens split across the models it used, evenly for want of a
 * per-model record, so a multi-model session still prices at the right total. */
const modelSlices = (s: SessionAgg): readonly [string, Tokens][] => {
  const models = s.models.length === 0 ? [UNKNOWN_MODEL] : uniqSorted(s.models);
  const slice = 1 / models.length;
  return models.map((model) => [
    model,
    mapTokens(tokensOf(s), (n) => n * slice),
  ]);
};

/** Estimated USD for one session, priced per model slice. */
const sessionCost = (s: SessionAgg): number =>
  sum(modelSlices(s), ([model, tokens]) =>
    estimateCost(tokens, priceFor(model).rates)
  );

/** Token counts, short enough to read inside a sentence. Locale-independent. */
const compact = (n: number): string => {
  const abs = Math.abs(n);
  if (abs >= BILLION) {
    return `${(n / BILLION).toFixed(2)}B`;
  }
  if (abs >= MILLION) {
    return `${(n / MILLION).toFixed(2)}M`;
  }
  if (abs >= THOUSAND) {
    return `${(n / THOUSAND).toFixed(1)}k`;
  }
  return String(Math.round(n));
};

/** A share, as a percentage with one decimal. */
const pct = (part: number, total: number): string =>
  `${(share(part, total) * PERCENT).toFixed(1)}%`;

/** An estimate, always marked as one. Never a locale format. */
const usd = (n: number): string => `~$${n.toFixed(2)} est`;

/** The rate table, echoed so a reader can check what produced the estimate. */
const pricingEcho = (): string => {
  const rows = sortedRecord(
    Object.entries(DEFAULT_PRICING).map(
      ([family, r]) =>
        [
          family,
          `${r.input}/${r.output}/${r.cacheWrite}/${r.cacheRead}`,
        ] as const
    )
  );
  return Object.entries(rows)
    .map(([family, rates]) => `${family} ${rates}`)
    .join("; ");
};

/** Note the rows a cap hid, so a capped table never reads as the whole corpus. */
const noteDropped = (args: {
  readonly ctx: FinishContext;
  readonly dropped: number;
  readonly what: string;
}): void => {
  if (args.dropped > 0) {
    args.ctx.caveat(`${args.dropped} more ${args.what} not shown`);
  }
};

/** Cost rows for one dimension, ranked by their own estimate. */
const costBlock = (args: {
  readonly aggs: ReadonlyMap<string, CostAgg>;
  readonly ctx: FinishContext;
  readonly limit: number;
  readonly name: (key: string) => string;
  readonly prefix: string;
  readonly usdOf: (key: string, agg: CostAgg) => number;
  readonly what: string;
}): readonly CostRow[] => {
  const { aggs, ctx, limit, name, prefix, usdOf, what } = args;
  const builds = [...aggs].map(([key, agg]) => {
    const est = usdOf(key, agg);
    return {
      key: `${prefix}:${key}`,
      est,
      row: {
        key: `${prefix}:${key}`,
        label: `${name(key)} (${usd(est)})`,
        sessions: agg.sessions,
        ...mapTokens(agg.tokens, Math.round),
      } satisfies CostRow,
    };
  });
  const ranked = rank(
    builds,
    compareKeyed<(typeof builds)[number]>((b) => b.est),
    limit
  );
  noteDropped({ ctx, dropped: ranked.dropped, what });
  return ranked.rows.map((b) => b.row);
};

/** The corpus panels. Session rows feed context, fan-out and cost; tool rows
 * feed the web panel through `arg`, which is already redacted. */
export const makeCorpusPanels: AccumulatorFactory<CorpusPanels> = () => {
  const sessions = new Map<string, SessionAgg>();
  const runCalls = new Map<string, RunCalls>();
  const web = new Map<string, WebAgg>();
  let webCalls = 0;

  const addSession = (fact: SessionFact): void => {
    const found = sessions.get(fact.sid);
    sessions.set(
      fact.sid,
      found ? mergeSession(found, fact) : sessionAggOf(fact)
    );
  };

  const addWeb = (fact: Extract<StatsFact, { row: "tool" }>): void => {
    if (!(WEB_TOOLS.has(fact.tool) && fact.arg)) {
      return;
    }
    webCalls += 1;
    const key = `${fact.tool} ${fact.arg}`;
    const found = web.get(key) ?? {
      tool: fact.tool,
      calls: 0,
      chars: 0,
      sessions: new Set<string>(),
      runs: new Set<string>(),
    };
    found.calls += 1;
    found.chars += fact.outChars;
    found.sessions.add(fact.sid);
    found.runs.add(fact.run ?? "");
    web.set(key, found);
  };

  const contextPanel = (
    ctx: FinishContext,
    all: readonly SessionAgg[]
  ): readonly ContextRow[] => {
    const totalRead = sum(all, (s) => s.cacheReadTokens);
    const head = Math.max(1, Math.ceil(all.length * CONCENTRATION_HEAD));
    const byRead = sortBy(
      all,
      compareKeyed<SessionAgg>((s) => s.cacheReadTokens)
    );
    const headRead = sum(byRead.slice(0, head), (s) => s.cacheReadTokens);
    if (totalRead > 0) {
      ctx.caveat(
        `Context: the top 1% of sessions (${head} of ${all.length}) hold ${pct(headRead, totalRead)} of ${compact(totalRead)} cache-read tokens — compact or split those sessions before tuning anything else`
      );
    }
    const ranked = rank(
      all,
      compareKeyed<SessionAgg>((s) => s.maxContextTokens),
      ctx.limit
    );
    noteDropped({ ctx, dropped: ranked.dropped, what: "context sessions" });
    return ranked.rows.map((s) => ({
      key: s.key,
      sid: s.sid,
      agent: s.agent,
      project: ctx.redact.text(s.project),
      kind: s.kind,
      maxContextTokens: s.maxContextTokens,
      cacheReadTokens: s.cacheReadTokens,
      cacheCreationTokens: s.cacheCreationTokens,
      outputTokens: s.outputTokens,
    }));
  };

  const fanoutPanel = (
    ctx: FinishContext,
    all: readonly SessionAgg[]
  ): readonly FanoutRow[] => {
    const agents = all.filter((s) => s.kind === "workflow-agent" && s.run);
    const byRun = new Map<
      string,
      { prefix: number; projects: Map<string, number>; sids: Set<string> }
    >();
    for (const s of agents) {
      const run = s.run ?? "";
      const found = byRun.get(run) ?? {
        prefix: 0,
        projects: new Map<string, number>(),
        sids: new Set<string>(),
      };
      found.prefix += s.firstTurnCacheCreation;
      found.projects.set(s.project, (found.projects.get(s.project) ?? 0) + 1);
      found.sids.add(s.sid);
      byRun.set(run, found);
    }
    const created = sum(agents, (s) => s.cacheCreationTokens);
    const prefix = sum(agents, (s) => s.firstTurnCacheCreation);
    if (created > 0) {
      ctx.caveat(
        `Fan-out: ${pct(prefix, created)} of workflow cache creation (${compact(prefix)} of ${compact(created)} tokens) is first-turn prefix, re-paid by every agent a run spawns — shrink the shared prefix or spawn fewer agents; filtered to ${agents.length} workflow-agent transcripts across ${byRun.size} runs`
      );
    }
    const rows = [...byRun].map(([run, agg]) => ({
      key: run,
      project: ctx.redact.text(topKey(agg.projects)),
      agents: agg.sids.size,
      prefixTokens: agg.prefix,
      toolCalls: runCalls.get(run)?.calls ?? 0,
      totalSec: round(runCalls.get(run)?.sec ?? 0),
    }));
    const ranked = rank(
      rows,
      compareKeyed<FanoutRow>((r) => r.prefixTokens),
      ctx.limit
    );
    noteDropped({ ctx, dropped: ranked.dropped, what: "workflow runs" });
    return ranked.rows;
  };

  const webPanel = (ctx: FinishContext): readonly WebRepeatRow[] => {
    const repeats = [...web]
      .filter(([, agg]) => agg.calls > 1)
      .map(([key, agg]) => ({
        key: ctx.redact.line(key),
        tool: agg.tool,
        calls: agg.calls,
        sessions: agg.sessions.size,
        chars: agg.chars,
        sameRun: agg.runs.size === 1 && !agg.runs.has(""),
      }));
    const inRun = sum(
      repeats.filter((r) => r.sameRun),
      (r) => r.calls - 1
    );
    if (repeats.length > 0) {
      ctx.caveat(
        `Web: ${inRun} of ${webCalls} web calls re-fetched a url or query already answered inside the same run — cache the fetch at run level; ${repeats.length} urls were fetched more than once overall`
      );
    }
    const ranked = rank(
      repeats,
      compareKeyed<WebRepeatRow>((r) => r.calls),
      ctx.limit
    );
    noteDropped({ ctx, dropped: ranked.dropped, what: "repeated web fetches" });
    return ranked.rows;
  };

  const costPanel = (
    ctx: FinishContext,
    all: readonly SessionAgg[]
  ): readonly CostRow[] => {
    const byKind = new Map<string, CostAgg>();
    const byProject = new Map<string, CostAgg>();
    const byModel = new Map<string, CostAgg>();
    const usdByKind = new Map<string, number>();
    const usdByProject = new Map<string, number>();
    const unpriced = new Set<string>();
    let multiModel = 0;

    const fold = (
      map: Map<string, CostAgg>,
      key: string,
      tokens: Tokens
    ): void => {
      const found = map.get(key) ?? { sessions: 0, tokens: noTokens() };
      found.sessions += 1;
      found.tokens = addTokens(found.tokens, tokens);
      map.set(key, found);
    };

    for (const s of all) {
      const tokens = tokensOf(s);
      fold(byKind, s.kind, tokens);
      fold(byProject, s.project, tokens);
      const cost = sessionCost(s);
      usdByKind.set(s.kind, (usdByKind.get(s.kind) ?? 0) + cost);
      usdByProject.set(s.project, (usdByProject.get(s.project) ?? 0) + cost);
      const slices = modelSlices(s);
      if (slices.length > 1) {
        multiModel += 1;
      }
      for (const [model, sliceTokens] of slices) {
        fold(byModel, model, sliceTokens);
        if (!priceFor(model).priced) {
          unpriced.add(model);
        }
      }
    }

    ctx.caveat(
      `Cost is an ESTIMATE, not a measurement: no transcript records per-call tokens. Rates per million tokens (input/output/cacheWrite/cacheRead USD): ${pricingEcho()}`
    );
    if (unpriced.size > 0) {
      ctx.caveat(
        `Cost: ${unpriced.size} model ids have no rate card and are estimated at ${FALLBACK_FAMILY} rates — ${uniqSorted(unpriced).join(", ")}`
      );
    }
    if (multiModel > 0) {
      ctx.caveat(
        `Cost by model splits each session's tokens evenly across the models it used, because a session fact records no per-model split; ${multiModel} of ${all.length} sessions used more than one model`
      );
    }

    return [
      ...costBlock({
        aggs: byKind,
        ctx,
        limit: byKind.size,
        name: (key) => key,
        prefix: "kind",
        usdOf: (key) => usdByKind.get(key) ?? 0,
        what: "session kinds",
      }),
      ...costBlock({
        aggs: byModel,
        ctx,
        limit: ctx.limit,
        name: (key) => (priceFor(key).priced ? key : `${key} (unpriced)`),
        prefix: "model",
        usdOf: (key, agg) => estimateCost(agg.tokens, priceFor(key).rates),
        what: "models",
      }),
      ...costBlock({
        aggs: byProject,
        ctx,
        limit: ctx.limit,
        name: (key) => ctx.redact.text(key),
        prefix: "project",
        usdOf: (key) => usdByProject.get(key) ?? 0,
        what: "projects",
      }),
    ];
  };

  return {
    id: "corpus-panels",
    add: (fact) => {
      if (fact.row === "session") {
        addSession(fact);
        return;
      }
      if (fact.run !== null) {
        const found = runCalls.get(fact.run) ?? { calls: 0, sec: 0 };
        found.calls += 1;
        found.sec += fact.durSec ?? 0;
        runCalls.set(fact.run, found);
      }
      if (fact.row === "tool") {
        addWeb(fact);
      }
    },
    finish: (ctx) => {
      const all = [...sessions.values()];
      return {
        context: contextPanel(ctx, all),
        fanout: fanoutPanel(ctx, all),
        web: webPanel(ctx),
        cost: costPanel(ctx, all),
      };
    },
    // The corpus panels own no drill key: `DrillTable` routes fail, time,
    // cluster and wait, and a panel row already carries its own identity.
    drill: () => [],
  };
};
