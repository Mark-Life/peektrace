/** The `stats drill` page header names which slice of the match set it shows. */
import { expect, test } from "bun:test";
import {
  type DrillHit,
  type DrillResult,
  STATS_SCHEMA_VERSION,
} from "@workspace/core/services/stats/schema";
import { renderDrill } from "../src/commands/stats";

const hit: DrillHit = {
  sid: "s1",
  agent: "claude",
  project: "demo",
  kind: "main",
  parent: null,
  seq: 1,
  toolUseId: null,
  ts: "2026-03-01T10:00:00.000Z",
  line: "bun test",
  durSec: 2,
  tool: "Bash",
  failed: true,
  pairToolUseId: null,
  pairLine: null,
};

const page = (offset: number, hits: readonly DrillHit[]): DrillResult => ({
  schemaVersion: STATS_SCHEMA_VERSION,
  table: "fail",
  key: "k",
  total: 12,
  offset,
  hits,
  paths: {},
});

const headerOf = (r: DrillResult) => renderDrill(r, true).split("\n")[0];

test("the first page keeps the short header", () => {
  expect(headerOf(page(0, [hit, hit]))).toBe("fail k: 2 of 12 calls");
});

test("a later page names its range", () => {
  expect(headerOf(page(5, [hit, hit]))).toBe("fail k: calls 6-7 of 12");
});

test("an offset past the end says so", () => {
  expect(headerOf(page(20, []))).toBe(
    "fail k: 0 of 12 calls (offset 20 is past the end)"
  );
});
