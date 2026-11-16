/**
 * Codex, Pi and OpenCode sessions for the `stats` fixture HOME, written from
 * the shell steps of the Claude edge transcript plus lowercase `edit` and
 * `web_search` calls, and the memory files each agent reads.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { message, open, part, session } from "./parity-opencode-records";
import { SHELL_STEPS, type Step } from "./parity-stats-fixtures";

const T0 = Date.parse("2026-10-20T09:00:00.000Z");
const STEP_MS = 5000;
const OC_STEP_MS = 7000;
const DEFAULT_GAP_SEC = 2;

const write = (path: string, body: string) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
};

const doc = (rows: readonly unknown[]) =>
  `${rows.map((r) => JSON.stringify(r)).join("\n")}\n`;

const iso = (t: number) => new Date(t).toISOString();
const gapMs = (s: Step) => (s.gap ?? DEFAULT_GAP_SEC) * 1000;

/** Shell steps, then a failed and a fixed `edit`, then a repeated search. */
const agentSteps = (): readonly Step[] => [
  ...SHELL_STEPS.filter((s) => s.tool === undefined),
  {
    tool: "edit",
    input: { path: "/w/a.ts", oldText: "x", newText: "y" },
    out: "Could not find the exact text",
    err: true,
  },
  {
    tool: "edit",
    input: { path: "/w/a.ts", oldText: "x\n", newText: "y" },
    out: "ok",
  },
  { tool: "web_search", input: { query: "effect schema" }, out: "results" },
  { tool: "web_search", input: { query: "effect schema" }, out: "again" },
];

type Shape = (s: Step, i: number) => unknown[];

/** One Codex response item per step: `name` with `args(cmd)` for shell
 * steps, `apply_patch` for edits, `web_search_call` for searches. */
const codexCall =
  (name: string, args: (cmd: string) => unknown): Shape =>
  (s, i) => {
    if (s.tool === "web_search") {
      return [
        {
          type: "web_search_call",
          call_id: `c${i}`,
          action: { query: s.input?.query },
        },
      ];
    }
    if (s.tool === "edit") {
      return [
        {
          type: "custom_tool_call",
          name: "apply_patch",
          call_id: `c${i}`,
          input:
            "*** Begin Patch\n*** Update File: /w/a.ts\n-x\n+y\n*** End Patch",
        },
      ];
    }
    return [
      {
        type: "function_call",
        name,
        call_id: `c${i}`,
        arguments: JSON.stringify(args(s.cmd ?? "")),
      },
    ];
  };

const codexRollout = (
  id: string,
  cwd: string,
  steps: readonly Step[],
  shape: Shape
) => {
  let t = T0;
  const item = (payload: unknown) => ({
    timestamp: iso(t),
    type: "response_item",
    payload,
  });
  const rows: unknown[] = [
    {
      timestamp: iso(t),
      type: "session_meta",
      payload: { id, cwd, cli_version: "0.9.0" },
    },
    {
      timestamp: iso(t),
      type: "turn_context",
      payload: { model: "gpt-5.5", cwd },
    },
    item({
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: "run the checks" }],
    }),
  ];
  steps.forEach((s, i) => {
    t += STEP_MS;
    rows.push(...shape(s, i).map(item));
    t += gapMs(s);
    rows.push(
      item({
        type:
          s.tool === "edit"
            ? "custom_tool_call_output"
            : "function_call_output",
        call_id: `c${i}`,
        output: s.out,
      })
    );
    rows.push({
      timestamp: iso(t),
      type: "event_msg",
      payload: {
        type: "token_count",
        info: {
          last_token_usage: {
            input_tokens: 1000 * (i + 1),
            cached_input_tokens: 500 * i,
            output_tokens: 20,
          },
          total_token_usage: { input_tokens: (1000 * (i + 1) * (i + 2)) / 2 },
        },
      },
    });
  });
  return doc(rows);
};

const piSession = (sid: string, cwd: string, steps: readonly Step[]) => {
  let t = T0;
  let parent: string | null = null;
  let n = 0;
  const rows: unknown[] = [
    { type: "session", version: 3, id: sid, timestamp: iso(t), cwd },
  ];
  const push = (r: Record<string, unknown>) => {
    const id = `e${n}`;
    n += 1;
    rows.push({ id, parentId: parent, timestamp: iso(t), ...r });
    parent = id;
  };
  push({
    type: "message",
    message: { role: "user", content: "run the checks" },
  });
  steps.forEach((s, i) => {
    t += STEP_MS;
    const name = s.tool ?? "bash";
    push({
      type: "message",
      message: {
        role: "assistant",
        model: i % 2 ? "claude-sonnet-4-5" : "claude-opus-4-5",
        usage: {
          input: 3,
          cacheRead: 1000 * (i + 1),
          cacheWrite: 200,
          output: 20,
        },
        content: [
          {
            type: "toolCall",
            id: `t${i}`,
            name,
            arguments: s.input ?? { command: s.cmd },
          },
        ],
      },
    });
    t += gapMs(s);
    push({
      type: "message",
      message: {
        role: "toolResult",
        toolCallId: `t${i}`,
        toolName: name,
        content: [{ type: "text", text: s.out }],
        isError: s.err === true,
      },
    });
  });
  return doc(rows);
};

/** Two sessions in one DB; ids carry no `msg_`/`prt_` prefix, so each
 * session's rows get their own. */
const writeOpencode = (
  dataDir: string,
  cwd: string,
  steps: readonly Step[]
) => {
  const d = open(join(dataDir, "opencode.db"));
  for (const sid of ["ses_stats_a", "ses_stats_b"]) {
    session(d, {
      id: sid,
      project_id: "p1",
      directory: cwd,
      title: "stats",
      version: "1.18.2",
      time_created: T0,
      time_updated: T0 + steps.length * OC_STEP_MS + STEP_MS,
    });
    message(d, sid, "mu", T0, { role: "user", time: { created: T0 } });
    part(d, sid, "mu", "pu", { type: "text", text: "run the checks" });
    steps.forEach((s, i) => {
      const tc = T0 + (i + 1) * OC_STEP_MS;
      message(d, sid, `ma${i}`, tc, {
        role: "assistant",
        modelID: "claude-sonnet-4-5",
        providerID: "anthropic",
        tokens: {
          input: 3,
          output: 20,
          reasoning: 0,
          cache: { read: 1000 * (i + 1), write: 200 },
        },
        time: { created: tc, completed: tc + gapMs(s) },
      });
      part(d, sid, `ma${i}`, `pt${i}`, {
        type: "tool",
        tool: s.tool === "web_search" ? "webfetch" : (s.tool ?? "bash"),
        callID: `call_${i}`,
        state: {
          status: s.err ? "error" : "completed",
          input: s.input ?? { command: s.cmd },
          output: s.out,
          time: { start: tc, end: tc + gapMs(s) },
        },
      });
    });
  }
  d.close();
};

const uuid = (n: number) =>
  `019f0000-0000-7000-8000-${String(n).padStart(12, "0")}`;

/** Codex rollouts in four call shapes, two Pi sessions and an OpenCode DB in
 * the default data dir, all run from `~/work/agents` (a git repo). */
export const buildAgentsStats = (home: string) => {
  const steps = agentSteps();
  const cwd = join(home, "work/agents");
  mkdirSync(join(cwd, ".git"), { recursive: true });
  write(
    join(cwd, "AGENTS.md"),
    "- Shell is zsh: quote every glob passed as a flag value.\n- jq is installed; there is no jqq binary.\n"
  );
  write(
    join(home, ".codex/AGENTS.md"),
    "- Never sleep N && cmd: use an until-loop.\n"
  );
  write(join(home, ".pi/agent/AGENTS.md"), "- rm absolute paths only.\n");
  write(
    join(home, ".config/opencode/AGENTS.md"),
    "- Read a file before you edit it.\n"
  );
  const shapes: [string, Shape][] = [
    ["shell", codexCall("shell", (c) => ({ command: ["bash", "-lc", c] }))],
    [
      "local_shell",
      codexCall("local_shell", (c) => ({ command: ["zsh", "-c", c] })),
    ],
    ["shell", codexCall("shell", (c) => ({ command: c.split(" ") }))],
    ["exec_command", codexCall("exec_command", (c) => ({ cmd: c }))],
  ];
  const codexDir = join(home, ".codex/sessions/2026/10/20");
  shapes.forEach(([, shape], i) => {
    const id = uuid(i + 1);
    write(
      join(codexDir, `rollout-2026-10-20T09-00-0${i}-${id}.jsonl`),
      codexRollout(id, cwd, steps, shape)
    );
  });
  const piDir = join(home, ".pi/agent/sessions/--work-agents--");
  write(
    join(piDir, "2026-10-20T09-00-00-000Z_pi-a.jsonl"),
    piSession("pi-a", cwd, steps)
  );
  write(
    join(piDir, "2026-10-20T09-00-00-001Z_pi-b.jsonl"),
    piSession("pi-b", cwd, steps.slice(0, 6))
  );
  writeOpencode(join(home, ".local/share/opencode"), cwd, steps);
};
