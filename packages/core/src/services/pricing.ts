/**
 * API list prices per model generation, and the per-turn cost they give.
 *
 * List prices in USD per million tokens, checked 2026-11-08 against
 * https://platform.claude.com/docs/en/about-claude/pricing and
 * https://developers.openai.com/api/docs/pricing (shut-down OpenAI models from
 * their per-model pages). No batch discount, fast mode, regional surcharge or
 * subscription terms. Sonnet 4.5 / 4 above 200k bill at the standard rate here:
 * the old premium is no longer on the page.
 */
import type { Turn } from "./sessions/schema";

/** USD per million tokens. `cacheWrite` is the 5-minute (or only) write rate. */
export interface ModelPrice {
  readonly cacheRead: number;
  readonly cacheWrite: number;
  /** Rate for 1-hour cache writes; absent means `cacheWrite` applies. */
  readonly cacheWrite1h?: number;
  readonly input: number;
  readonly output: number;
}

/** One rate card, with the higher tier a long prompt bills at. */
interface PriceCard extends ModelPrice {
  /** Whole request bills at `rates` when its prompt exceeds `overTokens`. */
  readonly long?: { readonly overTokens: number; readonly rates: ModelPrice };
  readonly provider: "anthropic" | "openai";
}

/** The token block a cost is computed from. */
export type CostTokens = Pick<
  Turn,
  | "cacheCreation1hTokens"
  | "cacheCreationTokens"
  | "cacheReadTokens"
  | "inputTokens"
  | "outputTokens"
>;

/** What a model id resolved to: a card and its table key, or zero rates. */
type PriceMatch =
  | {
      /** Table key that matched, e.g. `opus-4-8`. */
      readonly key: string;
      readonly priced: true;
      readonly rates: PriceCard;
    }
  | {
      readonly key?: undefined;
      readonly priced: false;
      readonly rates: ModelPrice;
    };

const PRICES_CHECKED = "2026-11-08";

const MILLION = 1e6;
const HAIKU_LONG_OVER = 100_000;
const OPENAI_LONG_OVER = 272_000;

const ZERO: ModelPrice = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

const anthropic = (rates: ModelPrice, long?: PriceCard["long"]): PriceCard => ({
  ...rates,
  provider: "anthropic",
  ...(long ? { long } : {}),
});

/**
 * An OpenAI card. Models with no listed write rate bill cache writes as plain
 * input; Codex reports no writes anyway. The long tier reuses the write rate.
 */
const openai = (args: {
  readonly input: number;
  readonly cached: number;
  readonly output: number;
  readonly write?: number;
  readonly long?: {
    readonly input: number;
    readonly cached: number;
    readonly output: number;
  };
}): PriceCard => {
  const cacheWrite = args.write ?? args.input;
  const base = {
    input: args.input,
    cacheRead: args.cached,
    output: args.output,
    cacheWrite,
  };
  if (!args.long) {
    return { ...base, provider: "openai" };
  }
  const { input, cached, output } = args.long;
  return {
    ...base,
    provider: "openai",
    long: {
      overTokens: OPENAI_LONG_OVER,
      rates: { input, cacheRead: cached, output, cacheWrite },
    },
  };
};

const FABLE_5_1 = anthropic({
  input: 10,
  cacheWrite: 12.5,
  cacheWrite1h: 20,
  cacheRead: 0.25,
  output: 50,
});
const FABLE_5 = anthropic({
  input: 10,
  cacheWrite: 12.5,
  cacheWrite1h: 20,
  cacheRead: 1,
  output: 50,
});
const OPUS_5_5 = anthropic({
  input: 4,
  cacheWrite: 5,
  cacheWrite1h: 8,
  cacheRead: 0.2,
  output: 20,
});
const OPUS_5 = anthropic({
  input: 5,
  cacheWrite: 6.25,
  cacheWrite1h: 10,
  cacheRead: 0.5,
  output: 25,
});
const OPUS_4 = anthropic({
  input: 15,
  cacheWrite: 18.75,
  cacheWrite1h: 30,
  cacheRead: 1.5,
  output: 75,
});
const SONNET_5_5 = anthropic({
  input: 2,
  cacheWrite: 2.5,
  cacheWrite1h: 4,
  cacheRead: 0.1,
  output: 10,
});
const SONNET_4 = anthropic({
  input: 3,
  cacheWrite: 3.75,
  cacheWrite1h: 6,
  cacheRead: 0.3,
  output: 15,
});
const HAIKU_5_5 = anthropic(
  {
    input: 0.1,
    cacheWrite: 0.125,
    cacheWrite1h: 0.2,
    cacheRead: 0.01,
    output: 0.5,
  },
  {
    overTokens: HAIKU_LONG_OVER,
    rates: {
      input: 0.5,
      cacheWrite: 0.625,
      cacheWrite1h: 1,
      cacheRead: 0.05,
      output: 2.5,
    },
  }
);

const GPT_5_3 = openai({ input: 1.75, cached: 0.175, output: 14 });
const GPT_5_1 = openai({ input: 1.25, cached: 0.125, output: 10 });
const GPT_5_MINI = openai({ input: 0.25, cached: 0.025, output: 2 });
const GPT_4_1 = openai({ input: 2, cached: 0.5, output: 8 });

const CARDS: readonly (readonly [string, PriceCard])[] = [
  ["fable-5-1", FABLE_5_1],
  ["mythos-5-1", FABLE_5_1],
  ["fable-5", FABLE_5],
  ["mythos-5", FABLE_5],
  ["opus-5-5", OPUS_5_5],
  ["opus-5", OPUS_5],
  ["opus-4-8", OPUS_5],
  ["opus-4-7", OPUS_5],
  ["opus-4-6", OPUS_5],
  ["opus-4-5", OPUS_5],
  ["opus-4-1", OPUS_4],
  ["opus-4", OPUS_4],
  ["sonnet-5-5", SONNET_5_5],
  [
    "sonnet-5",
    anthropic({
      input: 2,
      cacheWrite: 2.5,
      cacheWrite1h: 4,
      cacheRead: 0.2,
      output: 10,
    }),
  ],
  ["sonnet-4-6", SONNET_4],
  ["sonnet-4-5", SONNET_4],
  ["sonnet-4", SONNET_4],
  ["haiku-5-5", HAIKU_5_5],
  [
    "haiku-4-5",
    anthropic({
      input: 1,
      cacheWrite: 1.25,
      cacheWrite1h: 2,
      cacheRead: 0.1,
      output: 5,
    }),
  ],
  [
    "gpt-6-astra",
    openai({
      input: 10,
      cached: 1,
      output: 50,
      write: 12.5,
      long: { input: 20, cached: 2, output: 75 },
    }),
  ],
  [
    "gpt-6.1-sol",
    openai({
      input: 2,
      cached: 0.1,
      output: 10,
      write: 2.5,
      long: { input: 4, cached: 0.2, output: 15 },
    }),
  ],
  [
    "gpt-6.1-terra",
    openai({
      input: 1,
      cached: 0.05,
      output: 5,
      write: 1.25,
      long: { input: 2, cached: 0.1, output: 7.5 },
    }),
  ],
  [
    "gpt-6-sol",
    openai({
      input: 2,
      cached: 0.2,
      output: 10,
      write: 2.5,
      long: { input: 4, cached: 0.4, output: 15 },
    }),
  ],
  [
    "gpt-6-luna",
    openai({
      input: 0.1,
      cached: 0.01,
      output: 0.5,
      write: 0.125,
      long: { input: 0.2, cached: 0.02, output: 0.75 },
    }),
  ],
  [
    "gpt-5.6-sol",
    openai({
      input: 4,
      cached: 0.4,
      output: 20,
      write: 5,
      long: { input: 8, cached: 0.8, output: 30 },
    }),
  ],
  [
    "gpt-5.6-terra",
    openai({
      input: 2,
      cached: 0.2,
      output: 12,
      write: 2.5,
      long: { input: 4, cached: 0.4, output: 18 },
    }),
  ],
  [
    "gpt-5.6-luna",
    openai({
      input: 0.2,
      cached: 0.02,
      output: 1.2,
      write: 0.25,
      long: { input: 0.4, cached: 0.04, output: 1.8 },
    }),
  ],
  [
    "gpt-5.5",
    openai({
      input: 5,
      cached: 0.5,
      output: 30,
      long: { input: 10, cached: 1, output: 45 },
    }),
  ],
  [
    "gpt-5.4",
    openai({
      input: 2.5,
      cached: 0.25,
      output: 15,
      long: { input: 5, cached: 0.5, output: 22.5 },
    }),
  ],
  ["gpt-5.4-mini", openai({ input: 0.75, cached: 0.075, output: 4.5 })],
  ["gpt-5.4-nano", openai({ input: 0.2, cached: 0.02, output: 1.25 })],
  ["gpt-5.3-codex", GPT_5_3],
  ["gpt-5.2", GPT_5_3],
  ["gpt-5.2-codex", GPT_5_3],
  ["gpt-5.1", GPT_5_1],
  ["gpt-5.1-codex", GPT_5_1],
  ["gpt-5.1-codex-max", GPT_5_1],
  ["gpt-5.1-codex-mini", GPT_5_MINI],
  ["gpt-5", GPT_5_1],
  ["gpt-5-codex", GPT_5_1],
  ["gpt-5-mini", GPT_5_MINI],
  ["gpt-5-nano", openai({ input: 0.05, cached: 0.005, output: 0.4 })],
  ["codex-mini-latest", openai({ input: 1.5, cached: 0.375, output: 6 })],
  ["o3", GPT_4_1],
  ["o4-mini", openai({ input: 1.1, cached: 0.275, output: 4.4 })],
  ["gpt-4.1", GPT_4_1],
  ["gpt-4.1-mini", openai({ input: 0.4, cached: 0.1, output: 1.6 })],
];

/** Longest key first, so `opus-4-1` wins over `opus-4`. */
const PRICE_CARDS = [...CARDS].sort((a, b) => b[0].length - a[0].length);

/**
 * Newest generation per Anthropic family, for a bare alias such as
 * `claude-opus` or `sonnet-latest`. An id that names any version the table
 * lacks (`claude-3-opus`, `claude-opus-4-1-thinking`, an unseen `opus-6`)
 * stays unpriced rather than borrowing another generation's rates.
 */
const FAMILY_FALLBACK: readonly (readonly [string, string])[] = [
  ["opus", "opus-5-5"],
  ["sonnet", "sonnet-5-5"],
  ["haiku", "haiku-5-5"],
  ["fable", "fable-5-1"],
  ["mythos", "mythos-5-1"],
];

const BOUNDARY = new Set(["/", ".", "-", "_", ":"]);

const SUFFIX = String.raw`(?:-\d{8}|-\d{4}-\d{2}-\d{2}|@\d{8}|-v\d+(?::\d+)?|-latest)`;
/** Date stamps, Bedrock versions and `-latest` that may trail a model key. */
const ALLOWED_SUFFIX = new RegExp(`^${SUFFIX}*$`);
const TRAILING_SUFFIX = new RegExp(`${SUFFIX}*$`);
const DIGIT = /\d/;

/** True when `key` sits on a boundary in `id` and only allowed suffixes follow. */
const keyMatches = (id: string, key: string) => {
  let at = id.indexOf(key);
  while (at >= 0) {
    const before = at === 0 || BOUNDARY.has(id.charAt(at - 1));
    if (before && ALLOWED_SUFFIX.test(id.slice(at + key.length))) {
      return true;
    }
    at = id.indexOf(key, at + 1);
  }
  return false;
};

/** The family a bare, version-free alias names, e.g. `opus` for `claude-opus-latest`. */
const aliasFamily = (dashed: string) => {
  const stem = dashed.replace(TRAILING_SUFFIX, "");
  if (DIGIT.test(stem)) {
    return;
  }
  return FAMILY_FALLBACK.find(([family]) => {
    const at = stem.length - family.length;
    return (
      stem.endsWith(family) && (at === 0 || BOUNDARY.has(stem.charAt(at - 1)))
    );
  });
};

const cardByKey = new Map(PRICE_CARDS);

/**
 * Match a model id to its rate card: the most specific generation key first,
 * then a bare family alias. Anything else is unpriced with zero rates, so the
 * caller can name it instead of guessing.
 */
export const priceFor = (modelId: string): PriceMatch => {
  const raw = modelId.trim().toLowerCase().replace("[1m]", "");
  const dashed = raw.replaceAll(".", "-");
  for (const [key, card] of PRICE_CARDS) {
    const id = card.provider === "anthropic" ? dashed : raw;
    if (keyMatches(id, key)) {
      return { priced: true, rates: card, key };
    }
  }
  const alias = aliasFamily(dashed);
  const card = alias && cardByKey.get(alias[1]);
  if (alias && card) {
    return { priced: true, rates: card, key: alias[1] };
  }
  return { priced: false, rates: ZERO };
};

/**
 * Estimated USD for one token block at one rate card. 1-hour writes bill at
 * `cacheWrite1h` when the block reports the split; the rest at `cacheWrite`.
 */
export const estimateCost = (tokens: CostTokens, rates: ModelPrice): number => {
  const oneHour = Math.min(
    tokens.cacheCreation1hTokens ?? 0,
    tokens.cacheCreationTokens
  );
  return (
    (tokens.inputTokens * rates.input +
      tokens.outputTokens * rates.output +
      tokens.cacheReadTokens * rates.cacheRead +
      (tokens.cacheCreationTokens - oneHour) * rates.cacheWrite +
      oneHour * (rates.cacheWrite1h ?? rates.cacheWrite)) /
    MILLION
  );
};

/** A turn that billed nothing, e.g. Claude's `<synthetic>` placeholders. */
const isZeroUsage = (t: CostTokens) =>
  t.inputTokens === 0 &&
  t.outputTokens === 0 &&
  t.cacheReadTokens === 0 &&
  t.cacheCreationTokens === 0;

/**
 * Estimated USD for one model call, or `undefined` when its model is unpriced.
 * The long tier applies to the whole request when the prompt (input + cache
 * read + cache write) exceeds the card's threshold.
 */
export const turnCost = (
  turn: CostTokens & Pick<Turn, "model">
): number | undefined => {
  if (isZeroUsage(turn)) {
    return 0;
  }
  const match = priceFor(turn.model);
  if (!match.priced) {
    return;
  }
  const card = match.rates;
  const prompt =
    turn.inputTokens + turn.cacheReadTokens + turn.cacheCreationTokens;
  const rates =
    card.long && prompt > card.long.overTokens ? card.long.rates : card;
  return estimateCost(turn, rates);
};

/**
 * The cards a set of model ids used, as `key in/out/write/read` rows plus the
 * check date. Only used cards, so the echo stays short.
 */
export const pricingEcho = (modelIds: Iterable<string>): string => {
  const rows = new Map<string, string>();
  for (const id of modelIds) {
    const { key, rates: r } = priceFor(id);
    if (key) {
      rows.set(key, `${r.input}/${r.output}/${r.cacheWrite}/${r.cacheRead}`);
    }
  }
  const body = [...rows]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, rates]) => `${key} ${rates}`)
    .join("; ");
  return `${body || "no priced models"} (checked ${PRICES_CHECKED})`;
};
