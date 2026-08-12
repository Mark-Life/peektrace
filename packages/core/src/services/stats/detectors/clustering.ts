/**
 * Error clusters: the same failure, seen in more than one session.
 *
 * The vocabulary lives in `../normalize` — one module, two entry points, so the
 * command side and the error side can never drift apart. This file is the
 * ranking on top of it, and one fold: a fact keeps the quoted literal that names
 * the defect, `clusterKeyOf` masks it, and the row prints the literals back as
 * the detail. Without the fold a schema rejection splits one row per missing
 * field; without the literal the row says a property was missing and not which.
 *
 * Rows rank by rows spanned, then sessions, then the key. Never by cluster
 * count: the six masks move the count in both directions at once, so it
 * measures the masks rather than the corpus.
 */
import type { AccumulatorFactory, FinishContext, Tally } from "../accumulate";
import {
  addToTally,
  hitOf,
  sampleOf,
  samplesOf,
  tallyFor,
} from "../accumulate";
import { clusterKeyOf, detailsOf } from "../normalize";
import {
  bump,
  clip,
  collapseWs,
  compareClusterRows,
  compareFacts,
  rank,
  round,
  sortBy,
  topKey,
  uniqSorted,
} from "../rank";
import type { CallFact, ClusterRow, DrillHit } from "../schema";
import { isCallFact } from "../schema";

export type { NormalizedCommand } from "../normalize";
export { errorSignature, normalizeCommand } from "../normalize";

/** Chars a cluster label keeps. */
const LABEL_MAX = 120;

/** Mask noise a folded key can end on. */
const TRAILING_PUNCT = /[:,\s]+$/;

/** Distinct literals tallied per cluster, so one noisy key stays bounded. */
const DETAIL_KEYS = 32;

/** Literals printed beside the label. */
const DETAIL_SHOWN = 3;

/** Facts retained for one drill key. */
const DRILL_CAP = 500;

/** A one-line human label for a signature: no mask noise, no trailing colon. */
export const clusterLabel = (sig: string): string =>
  clip(collapseWs(sig).replace(TRAILING_PUNCT, ""), LABEL_MAX);

/** Per-cluster state beyond the shared tally. */
interface Cluster {
  /** Quoted literals the key masked away, counted. */
  readonly details: Map<string, number>;
  readonly tally: Tally;
  readonly tools: Map<string, number>;
}

const detailSuffix = (details: Map<string, number>) => {
  const top = sortBy(
    uniqSorted(details.keys()),
    (a, b) => (details.get(b) ?? 0) - (details.get(a) ?? 0)
  ).slice(0, DETAIL_SHOWN);
  return top.length > 0 ? ` — ${top.join(", ")}` : "";
};

const rowOf = (args: {
  readonly cluster: Cluster;
  readonly ctx: FinishContext;
  readonly key: string;
}): ClusterRow => {
  const { cluster, ctx, key } = args;
  const { tally } = cluster;
  return {
    key,
    label: ctx.redact.line(
      `${clusterLabel(key)}${detailSuffix(cluster.details)}`,
      LABEL_MAX
    ),
    tool: topKey(cluster.tools),
    rows: tally.rows,
    sessions: tally.sessions.size,
    projects: tally.projects.size,
    totalSec: round(tally.totalSec),
    samples: samplesOf(tally, ctx.sampleCap),
  };
};

/** Error clusters, ranked by rows then sessions spanned. */
export const makeClusterTable: AccumulatorFactory<readonly ClusterRow[]> = (
  opts
) => {
  const clusters = new Map<string, Cluster>();
  const tallies = new Map<string, Tally>();
  const drilled: CallFact[] = [];
  const wanted = opts.drill?.table === "cluster" ? opts.drill.key : null;

  const clusterFor = (key: string): Cluster => {
    const found = clusters.get(key);
    if (found) {
      return found;
    }
    const fresh: Cluster = {
      tally: tallyFor(tallies, key),
      tools: new Map(),
      details: new Map(),
    };
    clusters.set(key, fresh);
    return fresh;
  };

  return {
    id: "clusters",

    add: (fact) => {
      if (!(isCallFact(fact) && fact.sig)) {
        return;
      }
      const key = clusterKeyOf(fact.sig);
      const cluster = clusterFor(key);
      addToTally({
        tally: cluster.tally,
        fact,
        sample: sampleOf(fact, opts.redact.line(fact.sig)),
      });
      bump(cluster.tools, fact.tool);
      for (const detail of detailsOf(fact.sig)) {
        if (cluster.details.size < DETAIL_KEYS || cluster.details.has(detail)) {
          bump(cluster.details, detail);
        }
      }
      if (key === wanted && drilled.length < DRILL_CAP) {
        drilled.push(fact);
      }
    },

    finish: (ctx) => {
      const rows = [...clusters].map(([key, cluster]) =>
        rowOf({ key, cluster, ctx })
      );
      const ranked = rank(rows, compareClusterRows, ctx.limit);
      if (ranked.dropped > 0) {
        ctx.caveat(
          `${ranked.dropped} of ${ranked.total} error clusters not shown`
        );
      }
      return ranked.rows;
    },

    drill: (ctx): readonly DrillHit[] =>
      sortBy(drilled, compareFacts).map((fact) =>
        hitOf(fact, ctx.redact.line(fact.sig ?? ""))
      ),
  };
};
