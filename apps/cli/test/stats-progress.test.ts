/** The stderr scan line: quiet on warm runs, throttled, always cleared. */
import { expect, test } from "bun:test";
import { makeProgressSink } from "../src/commands/stats-progress";

const harness = () => {
  let t = 0;
  const out: string[] = [];
  const sink = makeProgressSink({
    now: () => t,
    write: (chunk) => out.push(chunk),
  });
  return {
    out,
    sink,
    at: (ms: number) => {
      t = ms;
    },
  };
};

test("a pass that ends inside the delay prints nothing, clear included", () => {
  const h = harness();
  h.at(100);
  h.sink.onFile(1, 2);
  h.at(200);
  h.sink.onFile(2, 2);
  h.sink.clear();
  expect(h.out).toEqual([]);
});

test("redraws at most every 100 ms, plus the final count", () => {
  const h = harness();
  h.at(300);
  h.sink.onFile(1, 2240);
  h.at(350);
  h.sink.onFile(2, 2240);
  h.at(400);
  h.sink.onFile(1240, 2240);
  h.at(410);
  h.sink.onFile(2240, 2240);
  expect(h.out).toEqual([
    "\rscanning 1/2,240 transcripts…",
    "\rscanning 1,240/2,240 transcripts…",
    "\rscanning 2,240/2,240 transcripts…",
  ]);
  h.sink.clear();
  h.sink.clear();
  expect(h.out.at(-1)).toBe("\r\x1b[2K");
  expect(h.out).toHaveLength(4);
});

test("a throwing writer never escapes the callback", () => {
  let t = 0;
  let calls = 0;
  const sink = makeProgressSink({
    now: () => t,
    write: () => {
      calls += 1;
      throw new Error("EPIPE");
    },
  });
  t = 1000;
  expect(() => sink.onFile(1, 1)).not.toThrow();
  expect(() => sink.clear()).not.toThrow();
  expect(calls).toBe(2);
});
