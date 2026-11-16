import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  FAKE_SECRETS,
  hideFakeSecrets,
  revealFakeSecrets,
} from "./parity-fake-secrets";
import {
  decodeText,
  encodeText,
  goldenLines,
  goldenText,
  leaks,
  missingGoldenLine,
  setOracleRoots,
} from "./parity-oracle";
import { canonStatsClock, maskStatsClock } from "./parity-stats-cases";

const ORACLE = join(import.meta.dir, "parity-oracle.ts");
const ignore = (_line: string) => undefined;

describe("placeholders", () => {
  test("encode then decode gives the text back", () => {
    setOracleRoots({ work: "/tmp/g/base", repo: "/r/repo", report: ignore });
    const text = "/tmp/g/base/home and /r/repo/apps and /tmp/g/base2";
    const encoded = encodeText(text);
    expect(encoded).toBe("<WORK>/home and <REPO>/apps and <WORK>2");
    expect(decodeText(encoded)).toBe(text);
  });
});

describe("golden files", () => {
  const store = new Map<string, unknown[]>([
    ["b", [{ x: 1 }, { x: 2 }]],
    ["a", ["one"]],
  ]);

  test("lines are sorted by key, then call order", () => {
    expect(goldenLines(store)).toEqual([
      '["a",0,"one"]',
      '["b",0,{"x":1}]',
      '["b",1,{"x":2}]',
    ]);
  });

  test("the file is plain text, one line per value", () => {
    expect(goldenText(goldenLines(store))).toBe(
      '["a",0,"one"]\n["b",0,{"x":1}]\n["b",1,{"x":2}]\n'
    );
  });

  test("leaks names the forbidden text found", () => {
    expect(leaks("at /Users/me/x", ["/Users/me", "/r/repo"])).toEqual([
      "/Users/me",
    ]);
    expect(leaks("<WORK>/x", ["/Users/me"])).toEqual([]);
    expect(
      leaks("cwd /Users/me/proj", ["/Users/me"], ["/Users/me/proj"])
    ).toEqual([]);
  });
});

/** Run a script against the oracle in `mode`, writing or reading `file`. */
const script = (mode: string, file: string, body: string) =>
  Bun.spawnSync(
    [
      "bun",
      "-e",
      `const o = await import(${JSON.stringify(ORACLE)});
       const lines = [];
       o.setOracleRoots({ work: "/w", repo: "/r", report: (l) => lines.push(l) });
       ${body}
       await o.finishOracle();
       console.log(JSON.stringify({ out, lines }));`,
    ],
    { env: { ...process.env, PARITY_ORACLE: mode, PARITY_GOLDENS: file } }
  );

const parsed = (r: ReturnType<typeof script>) =>
  JSON.parse(r.stdout.toString()) as { out: unknown; lines: string[] };

describe("record and replay", () => {
  test("replay returns what record kept, repeats in order, last one after", () => {
    const dir = mkdtempSync(join(tmpdir(), "oracle-"));
    const file = join(dir, "s.jsonl");
    try {
      const rec = script(
        "record",
        file,
        `let n = 0;
         const out = [
           o.tsValueSync("k", () => "/w/a" + (n += 1)),
           o.tsValueSync("k", () => "/w/a" + (n += 1)),
           await o.tsValue("err", async () => { throw new Error("boom"); }).catch((e) => e.message),
         ];`
      );
      expect(parsed(rec).out).toEqual(["/w/a1", "/w/a2", "boom"]);
      const text = readFileSync(file, "utf8");
      expect(text).toContain('["k",0,"<WORK>/a1"]');
      const rep = script(
        "replay",
        file,
        `const out = [
           o.tsValueSync("k", () => "live"),
           o.tsValueSync("k", () => "live"),
           o.tsValueSync("k", () => "live"),
           await o.tsValue("err", async () => "live").catch((e) => e.message),
           o.tsValueSync("nope", () => "live", { missing: "gone" }),
         ];`
      );
      const r = parsed(rep);
      expect(r.out).toEqual(["/w/a1", "/w/a2", "/w/a2", "boom", "gone"]);
      expect(r.lines).toEqual([missingGoldenLine("nope")]);
      expect(r.lines[0]).not.toContain("--write");
      const unused = parsed(
        script("replay", file, `const out = o.tsValueSync("k", () => 1);`)
      );
      expect(unused.lines).toEqual(["golden not used: err"]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("fake secrets are placeholders in the file, real text on replay", () => {
    const dir = mkdtempSync(join(tmpdir(), "oracle-"));
    const file = join(dir, "s.jsonl");
    const all = Object.values(FAKE_SECRETS).join(" | ");
    const value = JSON.stringify({ text: all, nested: JSON.stringify(all) });
    try {
      const rec = script(
        "record",
        file,
        `const out = o.tsValueSync(${JSON.stringify(`k ${FAKE_SECRETS.digitalocean}`)}, () => JSON.parse(${JSON.stringify(value)}));`
      );
      expect(parsed(rec).lines).toEqual([]);
      const text = readFileSync(file, "utf8");
      expect(text).toContain("<fake-secret:digitalocean>");
      for (const secret of Object.values(FAKE_SECRETS)) {
        expect(text).not.toContain(secret);
      }
      const rep = parsed(
        script(
          "replay",
          file,
          `const out = o.tsValueSync(${JSON.stringify(`k ${FAKE_SECRETS.digitalocean}`)}, () => "live");`
        )
      );
      expect(rep.out).toEqual(JSON.parse(value));
      expect(rep.lines).toEqual([]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("fake secrets", () => {
  test("hide then reveal gives the text back", () => {
    const text = `a ${Object.values(FAKE_SECRETS).join(" b ")} c`;
    const hidden = hideFakeSecrets(text);
    for (const secret of Object.values(FAKE_SECRETS)) {
      expect(hidden).not.toContain(secret);
    }
    expect(revealFakeSecrets(hidden)).toBe(text);
  });

  test("a written golden line holds no listed secret", () => {
    const store = new Map<string, unknown[]>([
      [`key ${FAKE_SECRETS.github}`, [{ out: Object.values(FAKE_SECRETS) }]],
    ]);
    const text = goldenText(goldenLines(store));
    for (const secret of Object.values(FAKE_SECRETS)) {
      expect(text).not.toContain(secret);
    }
  });

  test("the checked-in goldens hold no listed secret", () => {
    const repo = join(import.meta.dir, "../..");
    const files = Bun.spawnSync(["git", "ls-files", "-z", "tests/goldens"], {
      cwd: repo,
    })
      .stdout.toString()
      .split("\0")
      .filter((f) => f.endsWith(".jsonl"));
    for (const f of files) {
      const text = readFileSync(join(repo, f), "utf8");
      for (const secret of Object.values(FAKE_SECRETS)) {
        expect(text.includes(secret) ? `${f}: ${secret}` : "").toBe("");
      }
    }
  });
});

describe("stats clock canon", () => {
  const json = `{
  "schemaVersion": 3,
  "generatedAt": "2026-11-13T05:00:00.000Z",
  "window": {
    "fromIso": "2026-10-14T05:00:00.000Z",
    "toIso": "2026-11-13T05:00:00.000Z",
    "days": 30
  },
  "causes": [{ "key": "cause:x" }]
}
`;
  const run = (stdout: string) => ({ code: 0, stdout, stderr: "" });

  test("masks the same as the raw output and still parses", () => {
    const canon = canonStatsClock(run(json)).stdout;
    expect(maskStatsClock(canon)).toBe(maskStatsClock(json));
    expect(JSON.parse(canon).causes[0].key).toBe("cause:x");
    expect(canonStatsClock(run(canon)).stdout).toBe(canon);
  });

  test("a prompt's data range is masked", () => {
    const text = "Fix it.\n\nData (1926-12-08 → 2026-11-13):\n- corpus\n";
    expect(maskStatsClock(text)).toBe(
      "Fix it.\n\nData (<window>):\n- corpus\n"
    );
  });

  test("text output gets its window line masked", () => {
    const text = "peektrace stats  all window  2026-10-14 → 2026-11-13\nrest\n";
    const canon = canonStatsClock(run(text)).stdout;
    expect(canon).toBe(maskStatsClock(text));
    expect(maskStatsClock(canon)).toBe(maskStatsClock(text));
  });
});
