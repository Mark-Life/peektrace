import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import {
  type AgentId,
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "../../src/services/agents";
import {
  SessionsService,
  SessionsServiceLive,
} from "../../src/services/sessions/service";

/*
 * Synthetic Claude Code layout for a session that ran one workflow and one
 * plain Task subagent, which itself spawned a depth-2 subagent:
 *
 *   <slug>/<sid>.jsonl                                   main transcript
 *   <slug>/<sid>/workflows/wf_run-1.json                  run summary
 *   <slug>/<sid>/subagents/agent-plain.jsonl (+ .meta)    Task subagent
 *   <slug>/<sid>/subagents/agent-deep.jsonl  (+ .meta)    parentAgentId: plain
 *   <slug>/<sid>/subagents/workflows/wf_run-1/
 *       agent-wfa.jsonl (+ .meta, workflowPhase: survey)
 *       agent-wfb.jsonl (+ .meta, workflowPhase: build)
 *       agent-wfc.jsonl (+ .meta, workflowPhase: null)
 *       journal.jsonl
 *   <slug>/<sid>/subagents/workflows/wf_run-2/agent-orphan  no run json, no call
 *   <slug>/<sid>/workflows/wf_run-3.json + subagents/workflows/wf_run-3/agent-secret
 *       no call; name and phase hold a fake secret, for redaction
 *
 * Workflow agents' meta has no toolUseId. The run links back to the call only
 * through the main transcript: the Workflow tool_result's `toolUseResult.runId`
 * equals the `wf_*` dir name.
 */

const SLUG = "-Users-demo-wf";
const SID = "22222222-2222-4222-8222-222222222222";
const RUN = "wf_run-1";
const RUN_NO_JSON = "wf_run-2";
const RUN_SECRET = "wf_run-3";
const FAKE_KEY = `sk-ant-${"a".repeat(48)}`;
const WF_CALL = "toolu_workflow_1";
const TASK_CALL = "toolu_task_1";
const DEEP_CALL = "toolu_task_deep";

const line = (o: unknown) => `${JSON.stringify(o)}\n`;

const usage = {
  input_tokens: 10,
  cache_read_input_tokens: 0,
  cache_creation_input_tokens: 0,
  output_tokens: 5,
};

const assistant = (uuid: string, requestId: string, content: unknown[]) =>
  line({
    type: "assistant",
    uuid,
    sessionId: SID,
    timestamp: "2026-06-01T10:00:01.000Z",
    requestId,
    message: {
      id: `msg-${uuid}`,
      role: "assistant",
      model: "claude-opus-4",
      content,
      usage,
    },
  });

const toolResult = (
  uuid: string,
  id: string,
  text: string,
  toolUseResult?: unknown
) =>
  line({
    type: "user",
    uuid,
    sessionId: SID,
    timestamp: "2026-06-01T10:00:02.000Z",
    message: {
      role: "user",
      content: [{ type: "tool_result", tool_use_id: id, content: text }],
    },
    ...(toolUseResult ? { toolUseResult } : {}),
  });

const mainTranscript = [
  line({
    type: "user",
    uuid: "u0",
    sessionId: SID,
    cwd: "/Users/demo/wf",
    timestamp: "2026-06-01T10:00:00.000Z",
    message: { role: "user", content: "run the demo workflow" },
  }),
  assistant("a1", "req-1", [
    { type: "tool_use", id: WF_CALL, name: "Workflow", input: { script: "x" } },
  ]),
  toolResult("u1", WF_CALL, `Workflow started: ${RUN}`, {
    runId: RUN,
    taskId: "task-1",
    status: "started",
    workflowName: "demo",
  }),
  assistant("a2", "req-2", [
    {
      type: "tool_use",
      id: TASK_CALL,
      name: "Task",
      input: { description: "plain task", prompt: "p" },
    },
  ]),
  toolResult("u2", TASK_CALL, "done"),
].join("");

const subTranscript = (sid: string) =>
  [
    line({
      type: "user",
      uuid: `${sid}-u`,
      isSidechain: true,
      sessionId: SID,
      timestamp: "2026-06-01T10:00:03.000Z",
      message: { role: "user", content: "go" },
    }),
    line({
      type: "assistant",
      uuid: `${sid}-a`,
      isSidechain: true,
      sessionId: SID,
      timestamp: "2026-06-01T10:00:04.000Z",
      requestId: `req-${sid}`,
      message: {
        id: `msg-${sid}`,
        role: "assistant",
        model: "claude-opus-4",
        content: [{ type: "text", text: "ok" }],
        usage,
      },
    }),
  ].join("");

let root = "";
const projectsRoot = () => join(root, "projects");

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "pt-wf-subagents-"));
  const sessDir = join(projectsRoot(), SLUG, SID);
  const subDir = join(sessDir, "subagents");
  const runDir = join(subDir, "workflows", RUN);
  mkdirSync(runDir, { recursive: true });
  mkdirSync(join(sessDir, "workflows"), { recursive: true });

  writeFileSync(join(projectsRoot(), SLUG, `${SID}.jsonl`), mainTranscript);
  writeFileSync(
    join(sessDir, "workflows", `${RUN}.json`),
    JSON.stringify({
      runId: RUN,
      taskId: "task-1",
      workflowName: "demo",
      status: "completed",
      phases: [{ title: "survey" }, { title: "build" }],
      workflowProgress: [
        { type: "workflow_phase", index: 0, title: "survey" },
        {
          type: "workflow_agent",
          index: 0,
          agentId: "wfa",
          phaseIndex: 0,
          phaseTitle: "survey",
          label: "survey-a",
          state: "done",
        },
        { type: "workflow_phase", index: 1, title: "build" },
        {
          type: "workflow_agent",
          index: 1,
          agentId: "wfb",
          phaseIndex: 1,
          phaseTitle: "build",
          label: "build-b",
          state: "done",
        },
        {
          type: "workflow_agent",
          index: 2,
          agentId: "wfc",
          phaseIndex: 1,
          phaseTitle: "build",
          label: "build-c",
          state: "done",
        },
      ],
    })
  );

  const write = (dir: string, id: string, meta: Record<string, unknown>) => {
    writeFileSync(join(dir, `agent-${id}.jsonl`), subTranscript(id));
    writeFileSync(join(dir, `agent-${id}.meta.json`), JSON.stringify(meta));
  };
  write(subDir, "plain", {
    agentType: "general-purpose",
    description: "plain task",
    toolUseId: TASK_CALL,
    spawnDepth: 1,
  });
  // Depth 2: its toolUseId names a call inside agent-plain, not the main file.
  write(subDir, "deep", {
    agentType: "general-purpose",
    description: "nested task",
    toolUseId: DEEP_CALL,
    parentAgentId: "plain",
    spawnDepth: 2,
  });
  write(runDir, "wfa", {
    agentType: "workflow-agent",
    description: "survey-a",
    workflowPhase: "survey",
    spawnDepth: 1,
  });
  write(runDir, "wfb", {
    agentType: "workflow-agent",
    description: "build-b",
    workflowPhase: "build",
    spawnDepth: 1,
  });
  write(runDir, "wfc", {
    agentType: "workflow-agent",
    description: "build-c",
    workflowPhase: null,
    spawnDepth: 1,
  });
  writeFileSync(join(runDir, "journal.jsonl"), "");

  const orphanDir = join(subDir, "workflows", RUN_NO_JSON);
  mkdirSync(orphanDir, { recursive: true });
  write(orphanDir, "orphan", { agentType: "workflow-agent", spawnDepth: 1 });

  const secretDir = join(subDir, "workflows", RUN_SECRET);
  mkdirSync(secretDir, { recursive: true });
  writeFileSync(
    join(sessDir, "workflows", `${RUN_SECRET}.json`),
    JSON.stringify({
      runId: RUN_SECRET,
      workflowName: `leak ${FAKE_KEY}`,
      workflowProgress: [
        {
          type: "workflow_agent",
          agentId: "secret",
          phaseIndex: 0,
          phaseTitle: "unused",
        },
      ],
    })
  );
  write(secretDir, "secret", {
    agentType: "workflow-agent",
    workflowPhase: `phase ${FAKE_KEY}`,
    spawnDepth: 1,
  });
});

afterAll(() => {
  if (root) {
    rmSync(root, { recursive: true, force: true });
  }
});

const stubRoots = (id: AgentId): AgentRoots => ({
  id,
  home: join(root, "home"),
  layout: id === "claude" ? "claude-projects" : "none",
  projectsRoot: projectsRoot(),
  supported: id === "claude",
});

const agentStub = (): AgentRegistryShape => ({
  allowedRoots: [projectsRoot()],
  encodeSlug: (p) => p.replace(/[/.]/g, "-"),
  gitRoot: (cwd) => Effect.succeed(cwd),
  listProjectSlugs: () => Effect.succeed([SLUG]),
  listSessionFiles: (id) =>
    Effect.succeed(
      id === "claude"
        ? [
            {
              path: join(projectsRoot(), SLUG, `${SID}.jsonl`),
              id: SID,
              slug: SLUG,
            },
          ]
        : []
    ),
  loadTranscript: ({ ref }) =>
    Effect.sync(() => {
      const text = readFileSync(ref.path, "utf8");
      return { text, sizeBytes: Buffer.byteLength(text, "utf8"), mtimeMs: 0 };
    }),
  memoryDir: ({ slug }) => Effect.succeed(join(projectsRoot(), slug, "memory")),
  projectsRoot: () => Effect.succeed(projectsRoot()),
  roots: stubRoots,
  sessionsGlob: () => Effect.succeed(join(projectsRoot(), "**", "*.jsonl")),
});

const parse = (redact = false) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const svc = yield* SessionsService;
      return yield* svc.parse({ id: SID, redact });
    }).pipe(
      Effect.provide(
        SessionsServiceLive.pipe(
          Layer.provide(Layer.succeed(AgentRegistry, agentStub())),
          Layer.provide(BunFileSystem.layer)
        )
      )
    )
  );

const byId = (subs: readonly { readonly id: string }[], id: string) =>
  subs.find((s) => s.id === id) as Record<string, unknown> | undefined;

describe("workflow subagents", () => {
  test("every agent file is found, including those under workflows/wf_*", async () => {
    const parsed = await parse();
    expect(parsed.subagents.map((s) => s.id).sort()).toEqual([
      "agent-deep",
      "agent-orphan",
      "agent-plain",
      "agent-secret",
      "agent-wfa",
      "agent-wfb",
      "agent-wfc",
    ]);
  });

  test("a workflow agent links to the Workflow call that launched its run", async () => {
    const parsed = await parse();
    expect(byId(parsed.subagents, "agent-wfa")?.toolUseId).toBe(WF_CALL);
    expect(byId(parsed.subagents, "agent-wfb")?.toolUseId).toBe(WF_CALL);
  });

  test("a workflow agent carries its run id and phase", async () => {
    const parsed = await parse();
    expect(byId(parsed.subagents, "agent-wfa")).toMatchObject({
      workflowRunId: RUN,
      workflowName: "demo",
      workflowPhase: "survey",
      workflowPhaseIndex: 0,
    });
    expect(byId(parsed.subagents, "agent-wfb")).toMatchObject({
      workflowRunId: RUN,
      workflowName: "demo",
      workflowPhase: "build",
      workflowPhaseIndex: 1,
    });
  });

  test("a depth-2 subagent names the subagent that spawned it", async () => {
    const parsed = await parse();
    expect(byId(parsed.subagents, "agent-deep")?.parentAgentId).toBe(
      "agent-plain"
    );
    expect(
      byId(parsed.subagents, "agent-plain")?.parentAgentId
    ).toBeUndefined();
  });

  test("a null meta phase falls back to the run json phase", async () => {
    const parsed = await parse();
    expect(byId(parsed.subagents, "agent-wfc")).toMatchObject({
      workflowRunId: RUN,
      workflowPhase: "build",
      workflowPhaseIndex: 1,
      toolUseId: WF_CALL,
    });
  });

  test("a run dir with no run json keeps its run id and stays unlinked", async () => {
    const parsed = await parse();
    const orphan = byId(parsed.subagents, "agent-orphan");
    expect(orphan?.workflowRunId).toBe(RUN_NO_JSON);
    expect(orphan?.toolUseId).toBeUndefined();
    expect(orphan?.workflowPhase).toBeUndefined();
    expect(orphan?.workflowName).toBeUndefined();
  });

  test("a run with no matching Workflow result keeps toolUseId undefined", async () => {
    const parsed = await parse();
    const secret = byId(parsed.subagents, "agent-secret");
    expect(secret?.workflowRunId).toBe(RUN_SECRET);
    expect(secret?.toolUseId).toBeUndefined();
  });

  test("non-workflow agents gain no workflow keys", async () => {
    const parsed = await parse();
    const plain = byId(parsed.subagents, "agent-plain") ?? {};
    expect(Object.keys(plain).filter((k) => k.startsWith("workflow"))).toEqual(
      []
    );
  });

  test("redact covers workflow phase and name", async () => {
    const parsed = await parse(true);
    const secret = byId(parsed.subagents, "agent-secret");
    expect(secret?.workflowName).toBe("leak [REDACTED:anthropic-key]");
    expect(secret?.workflowPhase).toBe("phase [REDACTED:anthropic-key]");
  });
});
