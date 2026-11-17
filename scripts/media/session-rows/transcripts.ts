// Synthetic transcripts for make-home.ts. Each builder returns the file to
// write, relative to HOME, and its last line's time for the file's mtime.
import path from "node:path";

export interface TranscriptFile {
  readonly file: string;
  readonly text: string;
  readonly mtime: Date;
}

const SEC_MS = 1000;
const TURN_GAP_SEC = 20;
const TOOL_EVERY = 3;

const at = (start: string, sec: number) =>
  new Date(Date.parse(start) + sec * SEC_MS).toISOString();

const jsonl = (rows: readonly object[]) =>
  `${rows.map((r) => JSON.stringify(r)).join("\n")}\n`;

interface ClaudeSpec {
  readonly id: string;
  readonly cwd: string;
  readonly branch: string;
  readonly start: string;
  readonly turns: number;
  readonly title?: string;
  readonly model?: string;
  readonly tail?: string;
}

/** `turns` user/assistant pairs; every third assistant turn runs a tool. */
export const claude = (o: ClaudeSpec): TranscriptFile => {
  const model = o.model ?? "claude-opus-4-8";

  const meta = {
    cwd: o.cwd,
    gitBranch: o.branch,
    sessionId: o.id,
    version: "2.1.0",
  };

  const rows: object[] = [];
  let t = 0;

  for (let i = 0; i < o.turns; i += 1) {
    const tool = i % TOOL_EVERY === 0;
    const content: object[] = [{ text: `Working on step ${i}.`, type: "text" }];

    if (tool) {
      content.push({
        id: `toolu_${i}`,
        input: { command: "bun test" },
        name: "Bash",
        type: "tool_use",
      });
    }

    rows.push(
      {
        ...meta,
        message: {
          content:
            i === 0
              ? "Look at the checkout flow and fix what is broken."
              : `Next step ${i}.`,
          role: "user",
        },
        timestamp: at(o.start, t),
        type: "user",
        uuid: `${o.id}-u${i}`,
      },
      {
        ...meta,
        message: {
          content,
          id: `msg_${o.id.slice(0, 4)}_${i}`,
          model,
          role: "assistant",
          usage: {
            cache_creation_input_tokens: 400,
            cache_read_input_tokens: 8000 + i * 150,
            input_tokens: 20,
            output_tokens: 180,
          },
        },
        requestId: `req_${o.id.slice(0, 4)}_${i}`,
        timestamp: at(o.start, t + TURN_GAP_SEC),
        type: "assistant",
        uuid: `${o.id}-a${i}`,
      }
    );
    t += 2 * TURN_GAP_SEC;

    if (tool) {
      rows.push({
        ...meta,
        message: {
          content: [
            {
              content: "12 pass, 0 fail",
              is_error: false,
              tool_use_id: `toolu_${i}`,
              type: "tool_result",
            },
          ],
          role: "user",
        },
        timestamp: at(o.start, t),
        type: "user",
        uuid: `${o.id}-r${i}`,
      });
      t += 5;
    }
  }

  if (o.tail !== undefined) {
    rows.push({
      ...meta,
      message: { content: o.tail, role: "user" },
      timestamp: at(o.start, t),
      type: "user",
      uuid: `${o.id}-tail`,
    });
    t += 5;
  }

  if (o.title !== undefined) {
    rows.push({
      aiTitle: o.title,
      sessionId: o.id,
      timestamp: at(o.start, t),
      type: "ai-title",
    });
  }

  const slug = o.cwd.replaceAll(/[^A-Za-z0-9]/gu, "-");

  return {
    file: path.join(".claude/projects", slug, `${o.id}.jsonl`),
    mtime: new Date(at(o.start, t)),
    text: jsonl(rows),
  };
};

interface CodexSpec {
  readonly id: string;
  readonly cwd: string;
  readonly branch: string;
  readonly start: string;
  readonly turns: number;
  readonly model: string;
}

/** A Codex rollout: one user/assistant pair plus a token count per turn. */
export const codex = (o: CodexSpec): TranscriptFile => {
  const rows: object[] = [
    {
      payload: {
        cli_version: "0.134.0",
        cwd: o.cwd,
        git: { branch: o.branch, commit_hash: "abc1234" },
        id: o.id,
        model_provider: "openai",
        originator: "codex_cli_rs",
        source: "cli",
        timestamp: o.start,
      },
      timestamp: o.start,
      type: "session_meta",
    },
    {
      payload: { cwd: o.cwd, model: o.model, turn_id: "turn-1" },
      timestamp: at(o.start, 1),
      type: "turn_context",
    },
  ];

  let t = 2;

  for (let i = 0; i < o.turns; i += 1) {
    const total = 12_000 + i * 900;

    rows.push(
      {
        payload: {
          content: [
            {
              text:
                i === 0
                  ? "Add rate limits to the public API."
                  : `Continue with part ${i}.`,
              type: "input_text",
            },
          ],
          role: "user",
          type: "message",
        },
        timestamp: at(o.start, t),
        type: "response_item",
      },
      {
        payload: {
          content: [{ text: `Done with part ${i}.`, type: "output_text" }],
          phase: "final_answer",
          role: "assistant",
          type: "message",
        },
        timestamp: at(o.start, t + 10),
        type: "response_item",
      },
      {
        payload: {
          info: {
            last_token_usage: {
              cached_input_tokens: 9000,
              input_tokens: 12_000,
              output_tokens: 300,
              reasoning_output_tokens: 20,
              total_tokens: 12_300,
            },
            model_context_window: 258_400,
            total_token_usage: {
              cached_input_tokens: 9000 * (i + 1),
              input_tokens: total,
              output_tokens: 300 * (i + 1),
              reasoning_output_tokens: 20,
              total_tokens: total + 300,
            },
          },
          type: "token_count",
        },
        timestamp: at(o.start, t + 11),
        type: "event_msg",
      }
    );
    t += 30;
  }

  const day = o.start.slice(0, 10).replaceAll("-", "/");
  const stamp = o.start.slice(0, 19).replaceAll(":", "-");

  return {
    file: path.join(".codex/sessions", day, `rollout-${stamp}-${o.id}.jsonl`),
    mtime: new Date(at(o.start, t)),
    text: jsonl(rows),
  };
};

interface PiSpec {
  readonly id: string;
  readonly cwd: string;
  readonly start: string;
  readonly provider: string;
  readonly model: string;
  readonly turns: number;
  readonly prompt: string;
}

/** A Pi session: a model change, then user/assistant message pairs. */
export const pi = (o: PiSpec): TranscriptFile => {
  const rows: object[] = [
    { cwd: o.cwd, id: o.id, timestamp: o.start, type: "session", version: 3 },
    {
      id: "m0000000",
      modelId: o.model,
      parentId: null,
      provider: o.provider,
      timestamp: at(o.start, 0),
      type: "model_change",
    },
  ];

  let parent = "m0000000";
  let t = 1;

  for (let i = 0; i < o.turns; i += 1) {
    const u = `u${String(i).padStart(7, "0")}`;
    const a = `a${String(i).padStart(7, "0")}`;

    rows.push(
      {
        id: u,
        message: {
          content: [
            {
              text: i === 0 ? o.prompt : `Keep going, step ${i}.`,
              type: "text",
            },
          ],
          role: "user",
          timestamp: Date.parse(at(o.start, t)),
        },
        parentId: parent,
        timestamp: at(o.start, t),
        type: "message",
      },
      {
        id: a,
        message: {
          api: "openai-completions",
          content: [{ text: `Step ${i} done.`, type: "text" }],
          model: o.model,
          provider: o.provider,
          responseId: `resp_${i}`,
          role: "assistant",
          stopReason: "stop",
          timestamp: Date.parse(at(o.start, t + 8)),
          usage: {
            cacheRead: 4000 + i * 100,
            cacheWrite: 0,
            input: 30,
            output: 90,
            totalTokens: 4120 + i * 100,
          },
        },
        parentId: u,
        timestamp: at(o.start, t + 8),
        type: "message",
      }
    );
    parent = a;
    t += TURN_GAP_SEC;
  }

  const slug = `--${o.cwd.slice(1).replaceAll("/", "-")}--`;
  const stamp = o.start.replaceAll(/[:.]/gu, "-");

  return {
    file: path.join(".pi/agent/sessions", slug, `${stamp}_${o.id}.jsonl`),
    mtime: new Date(at(o.start, t - TURN_GAP_SEC + 8)),
    text: jsonl(rows),
  };
};
