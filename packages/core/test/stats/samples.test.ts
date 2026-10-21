/** The sample pool is cut in batches; the samples a row ships must be exactly
 * the ones a cut on every add would have kept. */
import { expect, test } from "bun:test";
import { addToTally, type Tally } from "../../src/services/stats/accumulate";
import { pickSamples } from "../../src/services/stats/rank";
import type { SampleRef } from "../../src/services/stats/schema";
import { bashFact } from "./facts";

const freshTally = (): Tally => ({
  rows: 0,
  flagged: 0,
  totalSec: 0,
  durations: [],
  sessions: new Set(),
  projects: new Set(),
  samples: [],
});

/** A deterministic pseudo-random stream, so the test never flakes. */
const lcg = (seed: number) => {
  let x = seed;
  return () => {
    x = (x * 1_103_515_245 + 12_345) % 2_147_483_648;
    return x;
  };
};

test("batch cuts keep the same top samples as a cut on every add", () => {
  const next = lcg(56);
  const tally = freshTally();
  let eager: SampleRef[] = [];
  for (let seq = 0; seq < 2000; seq++) {
    const fact = bashFact({ seq, sid: `s${next() % 7}` });
    const sample: SampleRef = {
      sid: fact.sid,
      agent: "claude",
      project: "p",
      kind: "main",
      parent: null,
      seq,
      line: "x".repeat(next() % 9),
    };
    addToTally({ tally, fact, sample });
    eager.push(sample);
    if (eager.length > 64) {
      eager = [...pickSamples(eager, 64)];
    }
  }
  expect(pickSamples(tally.samples, 3)).toEqual(pickSamples(eager, 3));
  expect(pickSamples(tally.samples, 64)).toEqual(pickSamples(eager, 64));
});
