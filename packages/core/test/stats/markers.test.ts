/** A cause mark carries the whole fix its row prints, sharpened the same way,
 * and the call index resolves a link to a call no mark sits on. */
import { describe, expect, test } from "bun:test";
import { CAUSES } from "../../src/services/stats/detectors/causes";
import { makeMarkerCollector } from "../../src/services/stats/markers";
import { bashFact, redact } from "./facts";

const collect = (facts: Parameters<typeof bashFact>[0][]) => {
  const collector = makeMarkerCollector({ redact, sid: "s1" });
  for (const over of facts) {
    collector.add(bashFact(over));
  }
  return collector;
};

describe("cause marks", () => {
  test("a long fix reaches the transcript whole", () => {
    const [mark] = collect([
      {
        seq: 0,
        toolUseId: "t0",
        cmd: "pkill -f srv",
        err: true,
        cause: "pkill-self-kill",
      },
    ]).finish();
    const fix = CAUSES["pkill-self-kill"].fix ?? "";
    expect(fix.length).toBeGreaterThan(160);
    expect(mark?.label.endsWith(fix)).toBe(true);
  });

  test("a missing binary names its own fix, as its row does", () => {
    const [mark] = collect([
      {
        seq: 0,
        toolUseId: "t0",
        cmd: "jq . f",
        err: true,
        cause: "command-not-found",
        causeArg: "jq",
      },
    ]).finish();
    expect(mark?.label).toBe("command not found (jq): install jq");
  });
});
