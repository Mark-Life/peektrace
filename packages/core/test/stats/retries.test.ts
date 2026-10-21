/** Retry pairs and the cause table, over synthetic facts. */
import { describe, expect, test } from "bun:test";
import { defaultOptions } from "../../src/services/stats/accumulate";
import {
  callFixOf,
  makeCauseTable,
} from "../../src/services/stats/detectors/retries";
import type {
  BashFact,
  CauseRow,
  StatsFact,
  ToolFact,
} from "../../src/services/stats/schema";
import { bashFact, contextOf, redact, sessionFact, toolFact } from "./facts";

const at = (sec: number) =>
  new Date(Date.parse("2026-08-01T10:00:00.000Z") + sec * 1000).toISOString();

const glob = (seq: number, over: Partial<BashFact> = {}): BashFact =>
  bashFact({
    seq,
    ts: at(seq * 5),
    req: `r${seq}`,
    ctxTok: 1000 * (seq + 1),
    failed: true,
    cmd: "grep -rn foo --include=*.ts src | head",
    cause: "zsh-glob-flag",
    causeArg: "--include=*.ts",
    focus: "grep -rn foo --include=*.ts src",
    ...over,
  });

const ok = (seq: number, over: Partial<BashFact> = {}): BashFact =>
  bashFact({
    seq,
    ts: at(seq * 5),
    req: `r${seq}`,
    ctxTok: 1000 * (seq + 1),
    cmd: "grep -rn foo --include='*.ts' src | head",
    ...over,
  });

const tableOf = (facts: readonly StatsFact[], drill: string | null = null) => {
  const table = makeCauseTable({
    ...defaultOptions(redact),
    drill: drill === null ? null : { table: "cause", key: drill },
  });
  for (const f of facts) {
    table.add(f);
  }
  return table;
};

const rowsOf = (facts: readonly StatsFact[]) =>
  tableOf(facts).finish(contextOf()).rows;

const row = (rows: readonly CauseRow[], key: string) =>
  rows.find((r) => r.key === key);

const GLOB = "cause:zsh-glob-flag";

describe("retry pairs", () => {
  test("a glob abort followed by the quoted command is one pair", () => {
    const r = row(rowsOf([glob(0), ok(1)]), GLOB);
    expect(r?.hits).toBe(1);
    expect(r?.flagged).toBe(0);
    expect(r?.pairs).toBe(1);
    expect(r?.retrySessions).toBe(1);
    expect(r?.retrySec).toBe(5);
    expect(r?.retryCtxTokens).toBe(2000);
    expect(r?.diffs).toEqual([
      { from: "--include=*.ts", to: "--include='*.ts'", pairs: 1 },
    ]);
    expect(r?.details).toEqual([{ value: "*.ts", hits: 1 }]);
    expect(r?.pairSamples[0]?.retry.seq).toBe(1);
  });

  test("parallel siblings of one model call never pair", () => {
    expect(row(rowsOf([glob(0), ok(1, { req: "r0" })]), GLOB)?.pairs).toBe(0);
    expect(
      row(
        rowsOf([glob(0, { req: null }), ok(1, { req: null, ts: at(0) })]),
        GLOB
      )?.pairs
    ).toBe(0);
  });

  test("a sidechain call never retries a main call of the same sid", () => {
    expect(row(rowsOf([glob(0), ok(1, { side: true })]), GLOB)?.pairs).toBe(0);
  });

  test("fail, fail, pass is one pair over two hits", () => {
    const r = row(rowsOf([glob(0), glob(1), ok(2)]), GLOB);
    expect(r?.hits).toBe(2);
    expect(r?.pairs).toBe(1);
    expect(r?.pairSamples[0]?.failed.seq).toBe(1);
  });

  test("one success is claimed once, by the latest similar seed", () => {
    const err = (seq: number, cmd: string) =>
      bashFact({
        seq,
        ts: at(seq),
        req: `r${seq}`,
        err: true,
        failed: true,
        cmd,
      });
    const rows = rowsOf([
      err(0, "aaaa bbbb"),
      err(1, "bbbb cccc"),
      bashFact({ seq: 2, ts: at(2), req: "r2", cmd: "aaaa bbbb cccc" }),
    ]);
    const r = row(rows, "cause:unclassified:Bash");
    expect(r?.hits).toBe(2);
    expect(r?.pairs).toBe(1);
    expect(r?.pairSamples[0]?.failed.seq).toBe(1);
  });

  test("parallel failed siblings each pair with one parallel fix", () => {
    const fails = [0, 1, 2].map((seq) => glob(seq, { req: "r1", ts: at(0) }));
    const fixes = [3, 4, 5].map((seq) => ok(seq, { req: "r2", ts: at(4) }));
    const r = row(rowsOf([...fails, ...fixes]), GLOB);
    expect(r?.hits).toBe(3);
    expect(r?.pairs).toBe(3);
  });

  test("a clipped command's cut word is never the learned change", () => {
    const words = Array.from({ length: 40 }, (_, i) => `w${i}`).join(" ");
    const full = `git add ${words} && git commit -m "docs: plan the providers beside Codex"`;
    const rows = rowsOf([
      bashFact({
        seq: 0,
        ts: at(0),
        req: "r0",
        err: true,
        failed: true,
        cmd: `${full.slice(0, full.length - 8)}…`,
      }),
      bashFact({ seq: 1, ts: at(5), req: "r1", cmd: `${full} && git push` }),
    ]);
    const r = row(rows, "cause:unclassified:Bash");
    expect(r?.pairs).toBe(1);
    expect(r?.diffs).toEqual([]);
  });

  test("a seed keeps its best retry, earliest on a tie", () => {
    const rows = rowsOf([
      glob(0),
      ok(1, { cmd: "grep -rn fo --include='*.ts' src/x" }),
      ok(2),
    ]);
    expect(row(rows, GLOB)?.pairSamples[0]?.retry.seq).toBe(2);
  });

  test("tsc fails, the agent edits, tsc passes: work, not a retry", () => {
    const rows = rowsOf([
      bashFact({
        seq: 0,
        err: true,
        failed: true,
        failReason: "tsc",
        cmd: "bun run typecheck",
      }),
      toolFact({ seq: 1, tool: "Edit", arg: "/p/a.ts" }),
      bashFact({ seq: 2, cmd: "bun run typecheck" }),
    ]);
    expect(rows).toEqual([]);
  });

  test("an unclassified failure with an edit in between is not paired", () => {
    const fail = bashFact({
      seq: 0,
      ts: at(0),
      err: true,
      failed: true,
      cmd: "make thing",
    });
    const pass = bashFact({ seq: 2, ts: at(2), cmd: "make thing" });
    const edited = rowsOf([
      fail,
      toolFact({ seq: 1, tool: "Write", ts: at(1) }),
      pass,
    ]);
    expect(row(edited, "cause:unclassified:Bash")).toBeUndefined();
    const read = rowsOf([
      fail,
      toolFact({ seq: 1, tool: "Read", ts: at(1) }),
      pass,
    ]);
    expect(row(read, "cause:unclassified:Bash")?.pairs).toBe(1);
  });

  test("a rejected call is never a hit, in either harness wording", () => {
    const rejected = (sig: string) =>
      toolFact({ seq: 0, tool: "Edit", err: true, failed: true, sig });
    for (const sig of [
      "The user doesn't want to proceed with this tool use.",
      "The user doesn't want to take this action right now.",
    ]) {
      expect(
        rowsOf([rejected(sig), toolFact({ seq: 1, tool: "Edit" })])
      ).toEqual([]);
    }
  });

  test("every agent's file edit stops an unclassified pair", () => {
    const fail = bashFact({
      seq: 0,
      ts: at(0),
      err: true,
      failed: true,
      tool: "bash",
      cmd: "make thing",
    });
    const pass = bashFact({
      seq: 2,
      ts: at(2),
      tool: "bash",
      cmd: "make thing",
    });
    const edits: readonly StatsFact[] = [
      toolFact({ seq: 1, tool: "edit", ts: at(1) }),
      toolFact({ seq: 1, tool: "write", ts: at(1) }),
      toolFact({ seq: 1, tool: "apply_patch", ts: at(1) }),
      bashFact({ seq: 1, tool: "bash", bin: "apply_patch", ts: at(1) }),
    ];
    const read = toolFact({ seq: 1, tool: "read", ts: at(1) });
    expect(
      row(rowsOf([fail, read, pass]), "cause:unclassified:bash")?.pairs
    ).toBe(1);
    for (const edit of edits) {
      expect(row(rowsOf([fail, edit, pass]), "cause:unclassified:bash")).toBe(
        undefined
      );
    }
  });

  test("a shell write between an unclassified failure and its re-run stops the pair", () => {
    const cmd = "cd /r && bun run check && git commit -m fix";
    const fail = bashFact({ seq: 0, ts: at(0), err: true, failed: true, cmd });
    const pass = bashFact({ seq: 2, ts: at(2), cmd });
    const middle = (line: string) =>
      bashFact({ seq: 1, ts: at(1), req: "r1", cmd: line });
    const pairsWith = (line: string) =>
      row(rowsOf([fail, middle(line), pass]), "cause:unclassified:Bash")
        ?.pairs ?? 0;
    for (const edit of [
      "sed -i 's/a/b/' /r/a.ts",
      "perl -pi -e 's/a/b/' /r/a.ts",
      "python3 - <<'EOF'\np='/r/a.ts'\nopen(p,'w').write('x')\nEOF",
      "cat > /r/a.ts <<'EOF'\nx\nEOF",
      "echo x | tee /r/a.ts",
    ]) {
      expect(pairsWith(edit)).toBe(0);
    }
    for (const read of [
      "sed -n '1,5p' /r/a.ts",
      "cat /r/a.ts",
      "git status > /dev/null",
    ]) {
      expect(pairsWith(read)).toBe(1);
    }
  });

  test("a failed call with no text to compare pairs with nothing", () => {
    const argless = (seq: number, tool: string, over: Partial<ToolFact> = {}) =>
      toolFact({ seq, tool, ts: at(seq), req: `r${seq}`, arg: null, ...over });
    for (const tool of ["exec_command", "mcp__foo__bar"]) {
      const rows = rowsOf([
        argless(0, tool, { err: true, failed: true, sig: "Exit code 1" }),
        argless(1, tool),
      ]);
      expect(row(rows, `cause:unclassified:${tool}`)).toBeUndefined();
    }
    // A StructuredOutput retry is any later StructuredOutput call.
    const schema = rowsOf([
      argless(0, "StructuredOutput", {
        err: true,
        failed: true,
        cause: "structured-output-schema",
      }),
      argless(1, "StructuredOutput"),
    ]);
    expect(row(schema, "cause:structured-output-schema")?.pairs).toBe(1);
  });

  test("a cause-seeded pair ignores an edit in between", () => {
    const rows = rowsOf([
      glob(0),
      toolFact({ seq: 1, tool: "Edit", ts: at(5) }),
      ok(2),
    ]);
    expect(row(rows, GLOB)?.pairs).toBe(1);
  });

  test("a retry more than ten minutes later is not a retry", () => {
    expect(row(rowsOf([glob(0), ok(1, { ts: at(601) })]), GLOB)?.pairs).toBe(0);
    expect(row(rowsOf([glob(0), ok(1, { ts: at(600) })]), GLOB)?.pairs).toBe(1);
  });

  test("only the next three calls are looked at", () => {
    const filler = (seq: number) =>
      toolFact({ seq, tool: "Read", ts: at(seq * 5), req: `r${seq}` });
    expect(
      row(rowsOf([glob(0), filler(1), filler(2), ok(3)]), GLOB)?.pairs
    ).toBe(1);
    expect(
      row(rowsOf([glob(0), filler(1), filler(2), filler(3), ok(4)]), GLOB)
        ?.pairs
    ).toBe(0);
  });

  test("parallel siblings of the failed call do not use up the window", () => {
    const sibling = (seq: number) =>
      toolFact({ seq, tool: "Read", ts: at(0), req: "r0" });
    expect(
      row(rowsOf([glob(0), sibling(1), sibling(2), sibling(3), ok(4)]), GLOB)
        ?.pairs
    ).toBe(1);
  });

  test("an escaped quote inside double quotes does not split the learned diff", () => {
    const cmd = 'grep -rn "key\\": 1" src --include=*.ts';
    const rows = rowsOf([
      glob(0, { cmd, focus: cmd }),
      ok(1, { cmd: 'grep -rn "key\\": 1" src --include=\'*.ts\'' }),
    ]);
    expect(row(rows, GLOB)?.diffs).toEqual([
      { from: "--include=*.ts", to: "--include='*.ts'", pairs: 1 },
    ]);
  });

  test("a missing binary retried by the same name learns no diff", () => {
    const rows = rowsOf([
      bashFact({
        ts: at(0),
        failed: true,
        cmd: "jq .x f",
        cause: "command-not-found",
        causeArg: "jq",
        focus: ".x f",
      }),
      bashFact({ seq: 1, ts: at(5), cmd: "jq .x f" }),
    ]);
    const r = row(rows, "cause:command-not-found");
    expect(r?.pairs).toBe(1);
    expect(r?.diffs).toEqual([]);
  });

  test("context tokens count once per model call; unknown ones are not costed", () => {
    const other = (seq: number, over: Partial<BashFact> = {}) =>
      glob(seq, {
        cmd: "find docs -name *.md",
        causeArg: "*.md",
        focus: "find docs -name *.md",
        ...over,
      });
    const rows = rowsOf([
      glob(0),
      other(1),
      ok(2, { req: "rX", ctxTok: 5000 }),
      ok(3, { req: "rX", ctxTok: 5000, cmd: "find docs -name '*.md'" }),
    ]);
    const r = row(rows, GLOB);
    expect(r?.pairs).toBe(2);
    expect(r?.tokensMeasured).toBe(2);
    expect(r?.retryCtxTokens).toBe(5000);
    const uncosted = row(rowsOf([glob(0), ok(1, { ctxTok: null })]), GLOB);
    expect(uncosted?.pairs).toBe(1);
    expect(uncosted?.tokensMeasured).toBe(0);
    expect(uncosted?.retryCtxTokens).toBe(0);
  });

  test("main and nested calls are split, and rates divide by shell calls", () => {
    const nested = { sid: "agent-1", kind: "subagent" as const };
    const rows = rowsOf([
      glob(0),
      ok(1),
      glob(0, nested),
      ok(1, nested),
      ok(2, nested),
      ok(3, nested),
    ]);
    const r = row(rows, GLOB);
    expect(r?.hitsBy).toEqual({ main: 1, nested: 1 });
    expect(r?.pairsBy).toEqual({ main: 1, nested: 1 });
    expect(r?.per1k).toEqual({ main: 500, nested: 250 });
    expect(r?.sessions).toBe(2);
  });

  test("a missing binary may be retried by any shell command", () => {
    const rows = rowsOf([
      bashFact({
        seq: 0,
        ts: at(0),
        err: true,
        failed: true,
        cmd: "jq .name package.json",
        cause: "command-not-found",
        causeArg: "jq",
        focus: ".name package.json",
      }),
      bashFact({ seq: 1, ts: at(5), cmd: "bun .name package.json" }),
    ]);
    const r = row(rows, "cause:command-not-found");
    expect(r?.pairs).toBe(1);
    expect(r?.diffs[0]).toEqual({ from: "jq", to: "bun", pairs: 1 });
    expect(r?.fix).toContain("install jq");
  });

  test("a binary that ran after its last not-found is installed: the fix skips it", () => {
    const miss = (seq: number, bin: string) =>
      bashFact({
        seq,
        ts: at(seq * 5),
        err: true,
        failed: true,
        cmd: `${bin} x`,
        cause: "command-not-found",
        causeArg: bin,
        focus: "x",
      });
    const fixOf = (facts: readonly StatsFact[]) =>
      row(rowsOf(facts), "cause:command-not-found")?.fix;
    // jq ran later, in a pipe under a wrapper; rsync never did.
    const ran = bashFact({
      seq: 9,
      ts: at(45),
      cmd: "curl -s u | timeout 5 jq .",
    });
    expect(fixOf([miss(0, "jq"), miss(1, "rsync"), ran])).toBe("install rsync");
    // A run before the last miss proves nothing; a failed run neither.
    expect(fixOf([ran, miss(10, "jq")])).toContain("install jq");
    expect(fixOf([miss(0, "jq"), bashFact({ ...ran, err: true })])).toContain(
      "install jq"
    );
    // Every binary installed since: nothing left to fix.
    expect(fixOf([miss(0, "jq"), ran])).toBeNull();
    // A binary the host has now is installed too, though nothing ran it.
    const table = tableOf([
      miss(0, "strings"),
      miss(1, "rsync"),
      miss(2, "rsync"),
    ]);
    expect(table.missingBins()).toEqual(["rsync", "strings"]);
    table.markInstalled(["strings"]);
    expect(
      table
        .finish(contextOf())
        .rows.find((r) => r.key === "cause:command-not-found")?.fix
    ).toBe("install rsync");
  });

  test("a missing binary with no focus learns no diff, not the retry's cd", () => {
    const rows = rowsOf([
      bashFact({
        ts: at(0),
        failed: true,
        cmd: "cd /p && bc -l x",
        cause: "command-not-found",
        causeArg: "bc",
        focus: null,
      }),
      bashFact({ seq: 1, ts: at(5), cmd: "cd /p && bc -l x" }),
    ]);
    const r = row(rows, "cause:command-not-found");
    expect(r?.pairs).toBe(1);
    expect(r?.diffs).toEqual([]);
  });

  test("a missing binary learns the binary the retry ran, not a subshell or builtin", () => {
    const failed = bashFact({
      ts: at(0),
      failed: true,
      cmd: "rsync -a src/ dst/",
      cause: "command-not-found",
      causeArg: "rsync",
      focus: "rsync -a src/ dst/",
    });
    const diffsOf = (cmd: string) =>
      row(
        rowsOf([failed, bashFact({ seq: 1, ts: at(5), cmd })]),
        "cause:command-not-found"
      )?.diffs;
    expect(diffsOf("(cd src/ dst/")).toEqual([]);
    expect(diffsOf("(cp -a src/ dst/)")).toEqual([
      { from: "rsync", to: "cp", pairs: 1 },
    ]);
    // A wrapper is not the binary, and a path prefix names the same one.
    expect(diffsOf("timeout 30 cp -a src/ dst/")).toEqual([
      { from: "rsync", to: "cp", pairs: 1 },
    ]);
    expect(diffsOf("nice -n 10 /bin/cp -a src/ dst/")).toEqual([
      { from: "rsync", to: "cp", pairs: 1 },
    ]);
    expect(diffsOf("/usr/bin/rsync -a src/ dst/")).toEqual([]);
  });

  test("a culprit missing from the clipped command learns no diff", () => {
    const rows = rowsOf([
      bashFact({
        ts: at(0),
        failed: true,
        cmd: "grep -rn foo src --exclude-dir=node_modules",
        cause: "zsh-glob-path",
        causeArg: "*.ts",
        focus: null,
      }),
      bashFact({
        seq: 1,
        ts: at(5),
        cmd: "grep -rn foo src --exclude-dir=node_module",
      }),
    ]);
    const r = row(rows, "cause:zsh-glob-path");
    expect(r?.pairs).toBe(1);
    expect(r?.diffs).toEqual([]);
  });

  test("a blocked sleep learns the wait that replaced it", () => {
    const blocked = bashFact({
      ts: at(0),
      failed: true,
      cmd: "sleep 200 && cat out.log",
      cause: "sleep-blocked",
      causeArg: "sleep 200",
      focus: "cat out.log",
    });
    const diffOf = (retry: Partial<BashFact>) =>
      row(
        rowsOf([blocked, bashFact({ seq: 1, ts: at(5), ...retry })]),
        "cause:sleep-blocked"
      )?.diffs;
    expect(
      diffOf({
        cmd: "until grep -q DONE out.log; do sleep 2; done; cat out.log",
        wait: "loop-poll",
      })
    ).toEqual([{ from: "sleep 200", to: "poll loop", pairs: 1 }]);
    expect(diffOf({ cmd: "cat out.log" })).toEqual([
      { from: "sleep 200", to: "(no wait)", pairs: 1 },
    ]);
    // The loop sits inside a quoted script, which the fact's wait shape masks.
    expect(
      diffOf({
        cmd: `timeout 100 bash -c 'until grep -q "D START" out.log; do sleep 3; done'; cat out.log`,
        wait: null,
      })
    ).toEqual([{ from: "sleep 200", to: "poll loop", pairs: 1 }]);
  });

  test("with no culprit, a word that only moved is never the learned change", () => {
    const rows = rowsOf([
      bashFact({
        ts: at(0),
        failed: true,
        req: "r0",
        cmd: "cd /p && rm -rf $D/out && bun run build --filter=inspector --force",
        cause: "rm-guard",
      }),
      bashFact({
        seq: 1,
        ts: at(5),
        req: "r1",
        cmd: "rm -rf /p/out && cd /p && bun run build --filter=inspector --force",
      }),
    ]);
    // `rm` and `cd` only moved; the rewrite is the target.
    expect(row(rows, "cause:rm-guard")?.diffs).toEqual([
      { from: "$D/out", to: "/p/out", pairs: 1 },
    ]);
  });

  test("a persisted output read back is a wasted call", () => {
    const rows = rowsOf([
      bashFact({
        seq: 0,
        ts: at(0),
        cmd: "git diff main",
        family: "git diff",
        cause: "output-too-large",
        causeArg: "b1x.txt",
      }),
      toolFact({
        seq: 1,
        ts: at(5),
        tool: "Read",
        arg: "/h/tool-results/b1x.txt",
      }),
    ]);
    const r = row(rows, "cause:output-too-large");
    expect(r?.failing).toBe(false);
    expect(r?.hits).toBe(1);
    expect(r?.flagged).toBe(0);
    expect(r?.pairs).toBe(1);
    expect(r?.details).toEqual([{ value: "git diff", hits: 1 }]);
    expect(r?.fix).toContain("git diff --stat");
  });

  test("a StructuredOutput re-send pairs, and the fix names the missing field", () => {
    const so = (seq: number, over: Partial<ToolFact> = {}) =>
      toolFact({
        seq,
        ts: at(seq),
        tool: "StructuredOutput",
        req: `r${seq}`,
        ...over,
      });
    const rows = rowsOf([
      so(0, {
        err: true,
        failed: true,
        cause: "structured-output-schema",
        causeArg: "severity",
      }),
      so(1),
    ]);
    const r = row(rows, "cause:structured-output-schema");
    expect(r?.pairs).toBe(1);
    expect(r?.kind).toBe("workflow");
    expect(r?.fix).toContain("'severity'");
  });

  test("a new session drops the open seeds of the last one", () => {
    expect(row(rowsOf([glob(0), ok(1, { sid: "s2" })]), GLOB)?.pairs).toBe(0);
  });

  test("an unclassified error with no retry is not a row", () => {
    expect(
      rowsOf([bashFact({ seq: 0, err: true, failed: true, cmd: "x" })])
    ).toEqual([]);
  });
});

describe("cause table", () => {
  test("the fd fix gives an fd binary on Debian/Ubuntu too", () => {
    const fix = callFixOf("command-not-found", "fd");
    expect(fix).toContain(
      "mkdir -p ~/.local/bin && ln -s $(command -v fdfind) ~/.local/bin/fd"
    );
    expect(fix).toContain("put ~/.local/bin on PATH in ~/.zshenv");
  });

  test("nvm is sourced, not put on PATH: it is a shell function", () => {
    const fix = callFixOf("command-not-found", "nvm");
    expect(fix).toContain("source $NVM_DIR/nvm.sh");
    expect(fix).not.toContain("put nvm on PATH");
  });

  test("rows rank by retry sessions, then sessions, then pairs", () => {
    const rows = rowsOf([
      bashFact({
        seq: 0,
        ts: at(0),
        err: true,
        failed: true,
        cmd: "pkill -f srv; run",
        cause: "pkill-self-kill",
      }),
      glob(1),
      ok(2),
    ]);
    expect(rows.map((r) => r.key)).toEqual([GLOB, "cause:pkill-self-kill"]);
  });

  test("the dirs a rule check reads are the cwds the hits ran in", () => {
    const table = tableOf([
      sessionFact({ cwd: "/work/b" }),
      sessionFact({ sid: "s0", cwd: "/work/a" }),
      glob(0),
    ]);
    expect(table.projectDirs()).toEqual(["/work/b"]);
  });

  test("an agent stored under one project reads the repo it ran in", () => {
    const table = tableOf([
      sessionFact({ sid: "lead", cwd: "/work/lead" }),
      glob(0, { sid: "lead" }),
      sessionFact({ sid: "agent", kind: "subagent", cwd: "/work/other" }),
      glob(1, { sid: "agent", kind: "subagent" }),
    ]);
    expect(table.projectDirs()).toEqual(["/work/lead", "/work/other"]);
    const { counts } = table.finish(contextOf());
    const reached = counts.since(GLOB, [
      { cover: { agent: "claude", dir: "/work/other" }, afterDay: "" },
    ]);
    expect(reached.hits).toEqual({ main: 0, nested: 1 });
  });

  test("a call whose session row was not seen falls back to its project's dir", () => {
    const table = tableOf([
      sessionFact({ sid: "s0", cwd: "/work/a" }),
      glob(0, { sid: "gone" }),
    ]);
    expect(table.projectDirs()).toEqual(["/work/a"]);
  });

  test("hits since a day count whole days strictly after it", () => {
    const table = tableOf([
      glob(0, { ts: "2026-08-01T10:00:00.000Z" }),
      glob(1, { ts: "2026-08-02T10:00:00.000Z", sid: "s2" }),
      ok(2, { ts: "2026-08-02T11:00:00.000Z", sid: "s3" }),
    ]);
    const { counts } = table.finish(contextOf());
    const claude = { agent: "claude", dir: null } as const;
    const since = counts.since(GLOB, [
      { cover: claude, afterDay: "2026-08-01" },
    ]);
    expect(since.hits).toEqual({ main: 1, nested: 0 });
    expect(since.calls).toEqual({ main: 2, nested: 0 });
  });

  test("a file counts only the calls it reaches: its agent, under its dir", () => {
    const table = tableOf([
      sessionFact({ sid: "a", project: "pa", cwd: "/work/a/sub" }),
      glob(0, { sid: "a", project: "pa" }),
      sessionFact({ sid: "b", project: "pb", cwd: "/work/b" }),
      glob(1, { sid: "b", project: "pb" }),
      sessionFact({ sid: "c", project: "pc", agent: "codex", cwd: "/x" }),
      glob(2, { sid: "c", project: "pc", agent: "codex" }),
    ]);
    const { counts } = table.finish(contextOf());
    const hitsOf = (
      cover: Parameters<typeof counts.since>[1][number]["cover"]
    ) => counts.since(GLOB, [{ cover, afterDay: "" }]).hits.main;
    expect(hitsOf({ agent: "claude", dir: null })).toBe(2);
    expect(hitsOf({ agent: "codex", dir: null })).toBe(1);
    expect(hitsOf({ agent: "claude", dir: "/work/a" })).toBe(1);
    expect(hitsOf({ agent: "claude", dir: "/work" })).toBe(2);
    expect(hitsOf({ agent: "claude", dir: "/work/ab" })).toBe(0);
    // A project file one agent loads never reaches another agent's calls.
    expect(hitsOf({ agent: "codex", dir: "/work" })).toBe(0);
    expect(hitsOf({ agent: "codex", dir: "/x" })).toBe(1);
  });

  test("a Codex project file reaches every cwd of its repo but a nested one's", () => {
    const codexAt = (sid: string, cwd: string, seq: number) => [
      sessionFact({ sid, project: sid, agent: "codex", cwd }),
      glob(seq, { sid, project: sid, agent: "codex" }),
    ];
    const table = tableOf([
      ...codexAt("a", "/r", 0),
      ...codexAt("b", "/r/pkg/deep", 1),
      ...codexAt("c", "/r/vendor/inner/src", 2),
    ]);
    const { counts } = table.finish(contextOf());
    const hitsOf = (codex: { exact: boolean; stops: readonly string[] }) =>
      counts.since(GLOB, [
        { cover: { agent: "codex", dir: "/r", codex }, afterDay: "" },
      ]).hits.main;
    expect(hitsOf({ exact: false, stops: [] })).toBe(3);
    expect(hitsOf({ exact: false, stops: ["/r/vendor/inner"] })).toBe(2);
    // Outside a repo, Codex reads the cwd alone.
    expect(hitsOf({ exact: true, stops: [] })).toBe(1);
  });

  test("a sourced global file reaches only the sessions of its source", () => {
    const table = tableOf([
      sessionFact({ sid: "d", cwd: "/w" }),
      glob(0, { sid: "d" }),
      sessionFact({ sid: "w", cwd: "/w", source: "work" }),
      glob(1, { sid: "w" }),
      glob(2, { sid: "w" }),
    ]);
    const { counts } = table.finish(contextOf());
    const hitsOf = (source: { id: string; isDefault: boolean }) =>
      counts.since(GLOB, [
        { cover: { agent: "claude", dir: null, source }, afterDay: "" },
      ]).hits.main;
    expect(hitsOf({ id: "work", isDefault: false })).toBe(2);
    expect(hitsOf({ id: "claude", isDefault: true })).toBe(1);
  });

  test("drill returns the pairs first, each with its retry's line", () => {
    const table = tableOf([glob(0), ok(1), glob(2, { sid: "s2" })], GLOB);
    table.finish(contextOf());
    const hits = table.drill(contextOf());
    expect(hits.map((h) => [h.sid, h.seq])).toEqual([
      ["s1", 0],
      ["s2", 2],
    ]);
    expect(hits[0]?.line).not.toContain(" → ");
    expect(hits[0]?.pairLine).toBe(ok(1).cmd);
    expect(hits[1]?.pairLine).toBeNull();
  });

  test("links carry the transcript's own call ids, the retry's included", () => {
    const table = tableOf(
      [glob(0, { toolUseId: "t0" }), ok(1, { toolUseId: "t1" })],
      GLOB
    );
    const [r] = table.finish(contextOf()).rows;
    expect(r?.samples[0]?.toolUseId).toBe("t0");
    expect(r?.pairSamples[0]?.retry.toolUseId).toBe("t1");
    const [hit] = table.drill(contextOf());
    expect([hit?.toolUseId, hit?.pairToolUseId]).toEqual(["t0", "t1"]);
  });
});
