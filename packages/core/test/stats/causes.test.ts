/** The cause catalog's detectors: what each one claims, and the transcript
 * greps, probes and echoes that print the same words without being one. */
import { describe, expect, test } from "bun:test";
import { CAUSES, detectCause } from "../../src/services/stats/detectors/causes";
import { FOCUS_SEP } from "../../src/services/stats/detectors/similarity";
import { extractFacts } from "../../src/services/stats/extract";
import { makeRedactor } from "../../src/services/stats/redact";
import type { BashFact } from "../../src/services/stats/schema";
import { CAUSE_IDS, parsedOf } from "./fixture-session";

/** The third argument documents the exit code; detection reads the output. */
const bash = (cmd: string, output: string, _failed = true) =>
  detectCause({ tool: "Bash", cmd, output });

describe("zsh glob aborts", () => {
  test("a flag glob, anywhere in the output, at any exit code", () => {
    const hit = bash(
      "grep -rn foo src --include=*.ts | head; echo done",
      "(eval):1: no matches found: --include=*.ts\ndone",
      false
    );
    expect(hit?.cause).toBe("zsh-glob-flag");
    expect(hit?.arg).toBe("--include=*.ts");
    expect(hit?.focus).toBe("grep -rn foo src --include=*.ts");
    expect(hit?.line).toBe("(eval):1: no matches found: --include=*.ts");
  });

  test("the value of a glob flag is a flag glob too", () => {
    expect(
      bash("rg -g *.ts needle", "zsh:1: no matches found: *.ts")?.cause
    ).toBe("zsh-glob-flag");
    expect(
      bash("find . -name *.md", "(eval):1: no matches found: *.md")?.cause
    ).toBe("zsh-glob-flag");
  });

  test("a path glob, even behind 2>/dev/null, is a path glob", () => {
    const hit = bash(
      "ls dir/*.x 2>/dev/null",
      "Exit code 1\n(eval):1: no matches found: dir/*.x"
    );
    expect(hit?.cause).toBe("zsh-glob-path");
    expect(hit?.arg).toBe("dir/*.x");
  });

  test("brace expansion reaches the message as its first branch", () => {
    expect(
      bash(
        "grep -rn x . --include=*.{ts,tsx}",
        "(eval):1: no matches found: --include=*.ts"
      )?.cause
    ).toBe("zsh-glob-flag");
  });

  test("a quoted pattern naming the culprit is not the glob that aborted", () => {
    expect(
      bash(
        'grep -h "no matches found: --include=*.ts" $TMPDIR/survey/lines.txt',
        "(eval):1: no matches found: --include=*.ts\n(eval):3: no matches found: x*"
      )
    ).toBeNull();
    expect(
      bash(
        "grep -rn x . --include='*.{ts,tsx}'",
        "(eval):1: no matches found: --include=*.ts"
      )
    ).toBeNull();
  });

  test("a grep of a transcript printing the message mid-line is not one", () => {
    expect(
      bash(
        "grep -rn 'no matches' ~/.claude/projects",
        "x.jsonl:12:(eval):1: no matches found: --include=*.ts"
      )
    ).toBeNull();
  });

  test("zsh names the culprit after ~ and $VAR expansion", () => {
    const tilde = bash(
      "ls ~/work/foo* 2>/dev/null",
      "Exit code 1\n(eval):1: no matches found: /home/u/work/foo*"
    );
    expect(tilde?.cause).toBe("zsh-glob-path");
    expect(tilde?.arg).toBe("~/work/foo*");
    expect(tilde?.focus).toBe("ls ~/work/foo* 2>/dev/null");
    expect(
      bash(
        "R=/repo; cat $R/apps/*.log",
        "(eval):1: no matches found: /repo/apps/*.log"
      )?.arg
    ).toBe("$R/apps/*.log");
    expect(
      bash("ls ~/other/foo*", "(eval):1: no matches found: /home/u/work/foo*")
    ).toBeNull();
  });

  test("a glob inside $(...) within double quotes is code, not quoted", () => {
    const hit = bash(
      'for s in A B; do echo "== $s: $(grep -rlw --include=*.ts "$s" apps)"; done',
      "(eval):1: no matches found: --include=*.ts"
    );
    expect(hit?.cause).toBe("zsh-glob-flag");
    expect(hit?.arg).toBe("--include=*.ts");
  });

  test("a heredoc body's apostrophe does not open a quote", () => {
    const cmd =
      "cat > notes.md <<EOF\n# the CLI's stream\nEOF\ngrep -rn x --include=*.ts .";
    expect(
      bash(cmd, "(eval):4: no matches found: --include=*.ts", false)?.cause
    ).toBe("zsh-glob-flag");
    expect(
      bash(
        "cat <<'EOF'\nls src/*.md\nEOF",
        "(eval):1: no matches found: src/*.md",
        false
      )
    ).toBeNull();
  });

  test("a culprit zsh prints unescaped matches its escaped spelling", () => {
    const hit = bash(
      "gh api repos/o/r/commits?sha=v2\\&per_page=1",
      "(eval):1: no matches found: repos/o/r/commits?sha=v2&per_page=1"
    );
    expect(hit?.cause).toBe("zsh-glob-path");
    expect(hit?.arg).toBe("repos/o/r/commits?sha=v2\\&per_page=1");
  });

  test("a line-start message whose culprit is not in the command is not one", () => {
    expect(
      bash(
        "jq -r .message.content t.jsonl",
        "(eval):1: no matches found: --include=*.ts"
      )
    ).toBeNull();
  });
});

describe("zsh equals abort", () => {
  test("zsh's own == not found line, for a word of the command", () => {
    const hit = bash(
      "ls; echo === tests ===; git status",
      "Exit code 1\nzsh:1: == not found"
    );
    expect(hit?.cause).toBe("zsh-equals-abort");
    expect(hit?.arg).toBe("===");
    expect(bash("echo ==foo", "(eval):1: =foo not found")?.cause).toBe(
      "zsh-equals-abort"
    );
  });

  test("the separator alone is not one: no_equals or bash prints it", () => {
    expect(
      bash("echo === tests ===; false", "Exit code 1\n=== tests ===")
    ).toBeNull();
    expect(bash('ls; echo "==="; git status', "")).toBeNull();
    expect(
      bash("grep -rn 'not found' log", "(eval):1: == not found", false)
    ).toBeNull();
  });

  test("a cause the output names keeps its own priority", () => {
    expect(
      bash("pkill -f srv.ts; sleep 1; echo ===; ls", "Exit code 144")?.cause
    ).toBe("pkill-self-kill");
    expect(
      bash(
        "echo === ; jq . f",
        "Exit code 127\n===\nbash: line 1: jq: command not found"
      )?.cause
    ).toBe("command-not-found");
  });
});

describe("pkill self-kill", () => {
  test("exit 144 with pkill -f in the same call", () => {
    expect(
      bash("pkill -f my-server; bun run dev", "Exit code 144")?.cause
    ).toBe("pkill-self-kill");
    expect(
      bash("pgrep -f srv | xargs kill; echo ok", "Exit code 144")?.cause
    ).toBe("pkill-self-kill");
  });

  test("-f after other flags, and pgrep -f as a loop source, are one too", () => {
    for (const cmd of [
      "pkill -u $(id -u) -f 'remote-debugging-port=9876'; rm -rf $D",
      'pkill -9 -f "bun mock.ts"',
      "for p in $(pgrep -f foo); do kill $p; done",
      "kill -9 $(pgrep -f foo)",
    ]) {
      expect(bash(cmd, "Exit code 144")?.cause).toBe("pkill-self-kill");
    }
    expect(bash("pkill -9 node; ls -f", "Exit code 144")).toBeNull();
  });

  test("a pkill -f on its own still kills the shell that holds its pattern", () => {
    expect(bash("pkill -f my-server", "Exit code 144")?.cause).toBe(
      "pkill-self-kill"
    );
    expect(CAUSES["pkill-self-kill"].fix).toContain("[p]attern");
  });

  test("a bracketed pattern still dies when the call repeats its plain text", () => {
    expect(
      bash("pkill -f '[b]un run dev'; bun run dev &", "Exit code 144")?.cause
    ).toBe("pkill-self-kill");
    const { fix, advice } = CAUSES["pkill-self-kill"];
    expect(fix).toContain("pkill -x NAME");
    expect(fix).toContain("pkill -f '[b]un run dev'; bun run dev & still dies");
    expect(advice).not.toContain("unless the pattern brackets");
  });

  test("ps | grep | kill reads the shell's own command line too", () => {
    for (const cmd of [
      "ps aux | grep '[v]ite' | awk '{print $2}' | xargs kill",
      "kill $(ps -eo pid,args | grep vite | awk '{print $1}')",
    ]) {
      expect(bash(cmd, "Exit code 144")?.cause).toBe("pkill-self-kill");
    }
    expect(bash("ps aux | grep vite", "Exit code 144")).toBeNull();
  });

  test("exit 144 without pkill is not one", () => {
    expect(bash("bun run dev", "Exit code 144")).toBeNull();
  });
});

describe("harness blocks", () => {
  test("a blocked sleep keeps the N and the blocked command as focus", () => {
    const hit = bash(
      "sleep 90; tail -5 /tmp/out.log",
      "<tool_use_error>Blocked: sleep 90 followed by: tail -5 /tmp/out.log. To wait for a condition, use Monitor.</tool_use_error>"
    );
    expect(hit?.cause).toBe("sleep-blocked");
    expect(hit?.arg).toBe("sleep 90");
    expect(hit?.focus).toBe("tail -5 /tmp/out.log");
  });

  test("the dangerous-rm guard", () => {
    expect(
      bash(
        "rm -f $D/*.txt",
        "Dangerous rm operation detected in `rm -f $D/*.txt`. The target is a shell variable expansion: when $D is unset..."
      )?.cause
    ).toBe("rm-guard");
  });

  test("a harness string that does not open the body is not one", () => {
    expect(
      bash("cat notes.md", "notes\nBlocked: sleep 90 followed by: tail")
    ).toBeNull();
  });
});

describe("command not found", () => {
  test("zsh and bash spellings, with the binary as culprit", () => {
    const zsh = bash(
      "cd /p && jq .name package.json",
      "Exit code 127\n(eval):1: command not found: jq"
    );
    expect(zsh?.cause).toBe("command-not-found");
    expect(zsh?.arg).toBe("jq");
    expect(zsh?.focus).toBe(".name package.json");
    expect(
      bash("strings bin | head", "bash: line 1: strings: command not found")
        ?.arg
    ).toBe("strings");
    expect(
      bash("jq . f", "Exit code 127\n/bin/bash: line 1: jq: command not found")
        ?.arg
    ).toBe("jq");
  });

  test("an env prefix or a wrapper is not part of the focus", () => {
    expect(
      bash("cd /p && FOO=1 sudo jq .x f", "(eval):1: command not found: jq")
        ?.focus
    ).toBe(".x f");
  });

  test("the fix names the startup file of either shell", () => {
    expect(CAUSES["command-not-found"].fix).toContain("~/.zshenv");
    expect(CAUSES["command-not-found"].fix).toContain("~/.bashrc");
    expect(CAUSES["command-not-found"].fix).toContain("export PATH");
  });

  test("an echo of the words, or a probe, is not one", () => {
    expect(
      bash('echo "command not found"', "command not found", false)
    ).toBeNull();
    expect(
      bash(
        "jq --version || echo missing",
        "(eval):1: command not found: jq\nmissing",
        false
      )
    ).toBeNull();
    expect(
      bash(
        "jq --version 2>/dev/null || echo missing",
        "(eval):1: command not found: jq\nmissing",
        false
      )
    ).toBeNull();
    for (const probe of [
      "pi --version 2>&1",
      "pi --version >&2",
      "pi -v 2>&1 | head",
      "pi --version &>/dev/null || echo no",
    ]) {
      expect(bash(probe, "(eval):1: command not found: pi", false)).toBeNull();
    }
  });

  test("a probe on the same line does not hide a real use", () => {
    const CNF = (bin: string) => `(eval):1: command not found: ${bin}`;
    expect(bash("command -v jq; jq .", CNF("jq"))?.cause).toBe(
      "command-not-found"
    );
    expect(bash("which psql; psql -h db -c 'select 1'", CNF("psql"))?.arg).toBe(
      "psql"
    );
    expect(bash("aws --version; aws s3 ls s3://b", CNF("aws"))?.arg).toBe(
      "aws"
    );
  });

  test("a binary that is not at a command position is not the culprit", () => {
    expect(bash("cat jq.md", "(eval):1: command not found: jq")).toBeNull();
  });
});

describe("persisted output", () => {
  const PERSISTED =
    "<persisted-output>\nOutput too large (51.6KB). Full output saved to: /h/.claude/projects/p/s/tool-results/b1x.txt\n\nPreview:";

  test("a shell call, at offset 0, culprit is the saved file's name", () => {
    const hit = detectCause({
      tool: "Bash",
      cmd: "git diff main",
      output: PERSISTED,
    });
    expect(hit?.cause).toBe("output-too-large");
    expect(hit?.arg).toBe("b1x.txt");
    expect(CAUSES["output-too-large"].failing).toBe(false);
  });

  test("a failure named in the preview wins", () => {
    expect(
      bash(
        "grep -rn x . --include=*.ts; git log",
        `${PERSISTED}\n(eval):1: no matches found: --include=*.ts\ncommit 1`,
        false
      )?.cause
    ).toBe("zsh-glob-flag");
  });

  test("a tool that is not a shell is not the shell-output row", () => {
    expect(
      detectCause({ tool: "WebFetch", cmd: "", output: PERSISTED })
    ).toBeNull();
  });

  test("not at offset 0 is not one", () => {
    expect(
      bash(
        "cat log",
        "line\n<persisted-output>\nOutput too large (1KB).",
        false
      )
    ).toBeNull();
  });
});

describe("file-editing tools", () => {
  const tool = (name: string, output: string) =>
    detectCause({ tool: name, cmd: "", output })?.cause ?? null;

  test("each harness message maps to its cause", () => {
    expect(
      tool(
        "Write",
        "<tool_use_error>File has not been read yet. Read it first before writing to it.</tool_use_error>"
      )
    ).toBe("read-before-write");
    expect(
      tool(
        "Edit",
        "<tool_use_error>File has been modified since read, either by the user or by a linter.</tool_use_error>"
      )
    ).toBe("stale-read");
    expect(
      tool(
        "Edit",
        "<tool_use_error>String to replace not found in file.</tool_use_error>"
      )
    ).toBe("edit-no-match");
    expect(
      tool(
        "Write",
        "<tool_use_error>String to replace not found</tool_use_error>"
      )
    ).toBeNull();
  });

  test("StructuredOutput names the missing field", () => {
    const hit = detectCause({
      tool: "StructuredOutput",
      cmd: "",
      output:
        "Output does not match required schema: /findings/0: must have required property 'severity'",
    });
    expect(hit?.cause).toBe("structured-output-schema");
    expect(hit?.arg).toBe("severity");
  });
});

describe("misc shell causes", () => {
  test("a scratch script that cannot resolve modules", () => {
    expect(
      bash(
        "bun /tmp/x/harness.ts",
        "error: Cannot find module 'effect' from '/tmp/x/harness.ts'"
      )?.cause
    ).toBe("scratch-module-resolution");
    expect(
      bash(
        "bun src/x.ts",
        "error: Cannot find module 'effect' from '/repo/src/template/x.ts'"
      )
    ).toBeNull();
  });

  test("node and tsx print the resolver error their own way", () => {
    expect(
      bash(
        "node /tmp/x.mjs",
        "Exit code 1\nError [ERR_MODULE_NOT_FOUND]: Cannot find package 'effect' imported from /tmp/x.mjs"
      )?.cause
    ).toBe("scratch-module-resolution");
    expect(
      bash(
        "node /tmp/x.cjs",
        "Exit code 1\nError: Cannot find module 'effect'\nRequire stack:\n- /tmp/x.cjs"
      )?.cause
    ).toBe("scratch-module-resolution");
    expect(
      bash(
        "npx tsx src/x.ts",
        "Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'effect' imported from /repo/src/x.ts"
      )
    ).toBeNull();
  });

  test("any scratch dir counts: /var/tmp, a TMPDIR, macOS T", () => {
    for (const path of [
      "/var/tmp/x.ts",
      "/home/u/.cache/agent-tmp/x.ts",
      "/var/folders/ab/cd/T/x.ts",
      "C:\\Users\\u\\AppData\\Local\\Temp\\x.ts",
    ]) {
      expect(
        bash(`bun ${path}`, `error: Cannot find module 'effect' from '${path}'`)
          ?.cause
      ).toBe("scratch-module-resolution");
    }
  });

  test("a log or a transcript quoting the resolver error is not one", () => {
    expect(
      bash(
        "cat /tmp/run/E.log | tail -20",
        "[12:01] child said: error: Cannot find module 'effect' from '/tmp/probe/x.ts'",
        false
      )
    ).toBeNull();
    expect(
      bash(
        "cat /tmp/run/E.log",
        "error: Cannot find module 'effect' from '/tmp/probe/x.ts'",
        false
      )
    ).toBeNull();
    expect(
      bash(
        "cd /tmp/probe && bun x.ts; echo done",
        "error: Cannot find module 'effect' from '/tmp/probe/x.ts'\ndone",
        false
      )?.cause
    ).toBe("scratch-module-resolution");
  });

  test("ugrep complexity", () => {
    expect(
      bash(
        "grep -E '(a|b){0,200}' f",
        "ugrep: error: error at position 11\n(a|b){0,200}\n    exceeds complexity limits"
      )?.cause
    ).toBe("ugrep-complexity");
  });
});

describe("catalog", () => {
  test("every cause has a fix, except the one that names none", () => {
    for (const id of CAUSE_IDS) {
      const spec = CAUSES[id];
      expect(spec.fix === null).toBe(spec.kind === "none");
      // A rule that can be found in memory names what to do when it still fires.
      expect(spec.probes.length === 0 || spec.advice !== null).toBe(true);
    }
  });
  test("zsh options go in ~/.zshenv, which every zsh reads", () => {
    for (const id of [
      "zsh-glob-flag",
      "zsh-glob-path",
      "zsh-equals-abort",
    ] as const) {
      expect(CAUSES[id].advice).toContain("~/.zshenv");
      expect(CAUSES[id].advice).not.toContain("~/.zshrc");
    }
  });

  test("the path-glob fix never probes with ls -d dir/*(N)", () => {
    // With nullglob an empty match leaves a bare `ls -d`, which lists the
    // cwd: a false hit.
    const fix = CAUSES["zsh-glob-path"].fix ?? "";
    expect(fix).not.toContain("(N)");
    expect(fix).not.toContain("ls -d");
    expect(fix).toContain("find");
  });
});

describe("extract", () => {
  test("an exit-0 chain hit fails the row, with the culprit line as signature", () => {
    const facts = extractFacts(
      parsedOf([
        {
          name: "Bash",
          input: {
            command: "grep -rn foo src --include=*.ts | head -5; echo end",
          },
          output:
            "src/a.ts:1:foo\n(eval):1: no matches found: --include=*.ts\nend",
          isError: false,
        },
      ])
    );
    const call = facts.find((f): f is BashFact => f.row === "bash");
    expect(call?.err).toBe(false);
    expect(call?.failed).toBe(true);
    expect(call?.cause).toBe("zsh-glob-flag");
    expect(call?.causeArg).toBe("--include=*.ts");
    expect(call?.focus?.split(FOCUS_SEP)).toEqual([
      "grep -rn foo src --include=*.ts",
    ]);
    expect(call?.sig).toContain("no matches found");
    expect(call?.benign).toBeNull();
  });

  test("a secret in the failing segment is redacted before quotes are stripped", () => {
    const key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
    const facts = extractFacts(
      parsedOf([
        {
          name: "Bash",
          input: {
            command: `cd /p && awsx configure set aws_secret_access_key "${key}"`,
          },
          output: "Exit code 127\n(eval):1: command not found: awsx",
          isError: true,
        },
      ])
    );
    const call = facts.find((f): f is BashFact => f.row === "bash");
    expect(call?.cause).toBe("command-not-found");
    expect(call?.focus).not.toBeNull();
    expect(call?.focus).not.toContain(key);
    expect(call?.cmd).not.toContain(key);
  });

  test("an unredacted pass keeps the same cause", () => {
    const raw = extractFacts({
      ...parsedOf([
        {
          name: "Bash",
          input: { command: "jq . f.json" },
          output: "Exit code 127\n(eval):1: command not found: jq",
          isError: true,
        },
      ]),
      redact: makeRedactor({ enabled: false }),
    });
    const call = raw.find((f): f is BashFact => f.row === "bash");
    expect(call?.cause).toBe("command-not-found");
  });
});

describe("rule probes", () => {
  const states = (line: string) =>
    CAUSE_IDS.filter((id) =>
      CAUSES[id].probes.some((probe) => probe.every((re) => re.test(line)))
    );

  test("a generic Read-first line is the read-before-write rule alone", () => {
    expect(states("Always use the Read tool before Edit or Write.")).toEqual([
      "read-before-write",
    ]);
  });

  test("the stale-read and edit-no-match rules need their own words", () => {
    expect(states("Re-read a file after the formatter touched it.")).toEqual([
      "stale-read",
    ]);
    expect(
      states("Copy old_string from the exact lines a Read printed.")
    ).toEqual(["edit-no-match"]);
  });
});
