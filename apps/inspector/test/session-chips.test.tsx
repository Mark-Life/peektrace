import { expect, test } from "bun:test";
import type { SampleRef } from "@workspace/core/services/stats/schema";
import { renderToStaticMarkup } from "react-dom/server";
import { chipLabelOf, sessionChips, titleKey } from "../src/lib/stats-format";
import {
  SampleTargets,
  SessionLinksView,
} from "../src/routes/stats/session-links";

const sample = (over: Partial<SampleRef> = {}): SampleRef => ({
  agent: "claude",
  kind: "subagent",
  line: "Bash: ls",
  parent: "aaaaaaaa-1111",
  project: "demo-app",
  seq: 0,
  sid: "sub-0",
  toolUseId: null,
  ts: null,
  ...over,
});

test("samples under one parent fold into one chip, order kept", () => {
  const groups = sessionChips([
    sample({ seq: 0, sid: "sub-0" }),
    sample({ parent: "bbbbbbbb-2222", seq: 1, sid: "sub-1" }),
    sample({ seq: 2, sid: "sub-2" }),
    sample({ seq: 3, sid: "sub-3" }),
  ]);
  expect(groups.map((g) => g.samples.length)).toEqual([3, 1]);
  expect(groups[0]?.first.sid).toBe("sub-0");
  expect(groups[1]?.first.parent).toBe("bbbbbbbb-2222");
});

test("a main sample and a subagent sample of one session share a chip", () => {
  const groups = sessionChips([
    sample({ kind: "main", parent: null, sid: "aaaaaaaa-1111" }),
    sample({ seq: 1, sid: "sub-1" }),
  ]);
  expect(groups).toHaveLength(1);
});

test("the same id under two agents stays two chips", () => {
  const groups = sessionChips([
    sample({ agent: "claude", parent: null, sid: "same-id" }),
    sample({ agent: "codex", parent: null, sid: "same-id" }),
  ]);
  expect(groups).toHaveLength(2);
});

test("a known title labels the chip, else project/id", () => {
  const ref = sample();
  const titles = new Map([
    [titleKey("claude", "aaaaaaaa-1111"), "  Fix the\n flaky   test "],
  ]);
  expect(chipLabelOf(ref, titles).label).toBe("Fix the flaky test");
  expect(chipLabelOf(ref, new Map()).label).toBe("demo-app/aaaaaaaa");
  expect(
    chipLabelOf(ref, new Map([[titleKey("claude", "aaaaaaaa-1111"), "  "]]))
      .label
  ).toBe("demo-app/aaaaaaaa");
});

test("a long title is cut with an ellipsis", () => {
  const titles = new Map([
    [titleKey("claude", "aaaaaaaa-1111"), "x".repeat(90)],
  ]);
  const { label } = chipLabelOf(sample(), titles);
  expect(label.length).toBeLessThan(50);
  expect(label.endsWith("…")).toBe(true);
});

test("a long title is not cut inside a surrogate pair", () => {
  const titles = new Map([
    [
      titleKey("claude", "aaaaaaaa-1111"),
      `${"a".repeat(38)}\u{1F600}tail more text`,
    ],
  ]);
  const { label } = chipLabelOf(sample(), titles);
  expect(label).toBe(`${"a".repeat(38)}\u{1F600}…`);
});

test("SessionLinksView shows one button and a count for same-parent samples", () => {
  const titles = new Map([
    [titleKey("claude", "aaaaaaaa-1111"), "Fix the test"],
  ]);
  const html = renderToStaticMarkup(
    <SessionLinksView
      detector={null}
      samples={[
        sample({ seq: 0, sid: "sub-0" }),
        sample({ seq: 1, sid: "sub-1" }),
        sample({ seq: 2, sid: "sub-2" }),
      ]}
      titles={titles}
    />
  );
  expect(html.match(/data-testid="stats-session-link"/g)).toHaveLength(1);
  expect(html).toContain("Fix the test");
  expect(html).toContain("×3");
});

test("a title keeps the short id beside it, so equal titles differ", () => {
  const titles = new Map([
    [titleKey("claude", "aaaaaaaa-1111"), "Fix tests"],
    [titleKey("claude", "bbbbbbbb-2222"), "Fix tests"],
  ]);
  const a = chipLabelOf(sample(), titles);
  const b = chipLabelOf(sample({ parent: "bbbbbbbb-2222" }), titles);
  expect(a.label).toBe(b.label);
  expect(a.id).toBe("aaaaaaaa");
  expect(b.id).toBe("bbbbbbbb");
  expect(chipLabelOf(sample(), new Map()).id).toBeNull();
  const html = renderToStaticMarkup(
    <SessionLinksView
      detector={null}
      samples={[sample(), sample({ parent: "bbbbbbbb-2222", seq: 1 })]}
      titles={titles}
    />
  );
  expect(html).toContain("aaaaaaaa");
  expect(html).toContain("bbbbbbbb");
});

test("a folded chip offers one target per sample", () => {
  const samples = [
    sample({ seq: 0, sid: "sub-0" }),
    sample({ seq: 1, sid: "sub-1" }),
    sample({ seq: 2, sid: "sub-2" }),
  ];
  const html = renderToStaticMarkup(
    <SampleTargets detector={null} samples={samples} />
  );
  expect(html.match(/data-testid="stats-session-target"/g)).toHaveLength(3);
  for (const s of samples) {
    expect(html).toContain(s.sid);
  }
});

test("a mixed group shows the subagent tag", () => {
  const html = renderToStaticMarkup(
    <SessionLinksView
      detector={null}
      samples={[
        sample({ kind: "main", parent: null, sid: "aaaaaaaa-1111" }),
        sample({ seq: 1, sid: "sub-1" }),
      ]}
      titles={new Map()}
    />
  );
  expect(html).toContain("subagent");
  expect(html).toContain('data-testid="stats-session-more"');
});
