/**
 * The labelled oracle for retry pairs: 40 hand-labelled fail → next-call pairs,
 * stratified by similarity bin, kept outside the repo because they hold real
 * commands. Runs only when `PEEKTRACE_RETRY_SAMPLE` names the JSONL file
 * (`{tool, fail_arg, fail_res, retry_arg, retry_ok, sim, label}` per line).
 *
 * Each pair is replayed as a two-call transcript through the real extractor,
 * so the gates see the clipped, redacted, whitespace-collapsed forms a fact
 * stores — not the raw text the #56 measurement scored.
 */
import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { makePairer } from "../../src/services/stats/detectors/pairing";
import { extractFacts } from "../../src/services/stats/extract";
import { makeRedactor } from "../../src/services/stats/redact";
import { refOf, type Step, transcriptOf } from "./fixture-session";

const SAMPLE = process.env.PEEKTRACE_RETRY_SAMPLE ?? "";
const PRESENT = SAMPLE !== "" && existsSync(SAMPLE);

interface LabelledPair {
  readonly fail_arg: string;
  readonly fail_res: string;
  readonly label: "RETRY" | "NOT";
  readonly retry_arg: string;
  readonly retry_ok: boolean;
  readonly sim: number;
  readonly tool: string;
}

const load = (): readonly LabelledPair[] =>
  PRESENT
    ? readFileSync(SAMPLE, "utf8")
        .trim()
        .split("\n")
        .map((l) => JSON.parse(l) as LabelledPair)
    : [];

const inputOf = (tool: string, arg: string): Record<string, unknown> => {
  if (tool === "Bash") {
    return { command: arg };
  }
  try {
    return JSON.parse(arg) as Record<string, unknown>;
  } catch {
    return { raw: arg };
  }
};

const FAIL_OPEN = /^(?:Exit code [1-9]|<tool_use_error>)/;

/** True when the replayed pair survives every gate the report applies. */
const paired = (p: LabelledPair, i: number): boolean => {
  const steps: Step[] = [
    {
      name: p.tool,
      input: inputOf(p.tool, p.fail_arg),
      output: p.fail_res,
      isError: FAIL_OPEN.test(p.fail_res) || p.tool !== "Bash",
    },
    {
      name: p.tool,
      input: inputOf(p.tool, p.retry_arg),
      output: "ok",
      isError: !p.retry_ok,
    },
  ];
  const sid = `pair-${i}`;
  const facts = extractFacts({
    ref: refOf(sid),
    parsed: parseClaudeSession({
      text: transcriptOf(steps),
      path: `/fixture/${sid}.jsonl`,
      sessionId: sid,
    }),
    project: "proj",
    redact: makeRedactor({ enabled: true }),
  });
  let found = false;
  const pairer = makePairer(() => {
    found = true;
  });
  for (const fact of facts) {
    pairer.step(fact);
  }
  pairer.flush();
  return found;
};

describe.skipIf(!PRESENT)("retry pairs against the labelled sample", () => {
  const sample = load();
  const scored = sample.map((p, i) => ({ p, hit: paired(p, i) }));
  const tp = scored.filter((s) => s.hit && s.p.label === "RETRY").length;
  const fp = scored.filter((s) => s.hit && s.p.label === "NOT").length;

  test("the sample is the one the rates were measured on", () => {
    expect(sample).toHaveLength(40);
    expect(sample.filter((p) => p.label === "RETRY")).toHaveLength(28);
  });

  test("the shipped gates keep their measured precision", () => {
    // 21 of 23 kept pairs are real retries (91%); the #56 measurement's 23 of 26 was
    // scored on raw text, before the clip and the whitespace collapse.
    expect({ tp, fp }).toEqual({ tp: 21, fp: 2 });
    expect(tp / (tp + fp)).toBeGreaterThanOrEqual(0.85);
  });
});
