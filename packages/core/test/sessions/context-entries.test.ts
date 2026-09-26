/** System prompts and other context-loaded entries, per agent.
 *
 * Each fixture carries the records that load text into the model window beyond
 * plain prompts and tool calls: Codex `base_instructions` and multi-part
 * injected messages, Pi system messages / compaction / custom entries, and
 * Claude `prompt_snapshot` / `rendered` attachments. The tests pin how each
 * surfaces in the timeline and that the system prompt stays inside the measured floor.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { eventBadgeLabel } from "../../src/services/sessions/labels";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { parseCodexSession } from "../../src/services/sessions/parsers/codex";
import { parsePiSession } from "../../src/services/sessions/parsers/pi";

const fixture = (rel: string) => {
  const path = join(import.meta.dir, "../fixtures", rel);
  return {
    path,
    text: readFileSync(path, "utf8"),
    sessionId: "fallback",
    slug: "",
  };
};

const codex = () =>
  parseCodexSession(fixture("sessions-codex/rollout-context.jsonl"));
const pi = () => parsePiSession(fixture("sessions-pi/pi-context.jsonl"));
const claude = () =>
  parseClaudeSession(fixture("sessions-claude/prompt-snapshot.jsonl"));

type Parsed = ReturnType<typeof pi>;
const withoutPrompt = (p: Parsed): Parsed => ({
  ...p,
  events: p.events.filter((e) => e.kind !== "system-prompt"),
});

describe("codex context entries", () => {
  test("base_instructions and dynamic tools become the system prompt", () => {
    const sys = codex().events[0];
    expect(sys?.kind).toBe("system-prompt");
    expect(sys?.attachmentType).toBe("base_instructions");
    expect(sys?.body).toContain("You are Codex, a coding agent.");
    expect(sys?.body).toContain("## dynamic tools (1)");
    expect(sys?.body).toContain("open_panel");
    expect(sys && eventBadgeLabel(sys)).toBe("system prompt");
  });

  test("developer parts are split and labelled by tag", () => {
    const dev = codex().events.filter((e) => e.index === 2);
    expect(dev.map((e) => e.attachmentType)).toEqual([
      "permissions_instructions",
      "skills_instructions",
    ]);
    expect(dev.map((e) => e.loadedCategory)).toEqual(["other", "skills"]);
  });

  test("AGENTS.md + environment is context, not a user prompt", () => {
    const p = codex();
    const injected = p.events.filter((e) => e.index === 3);
    expect(injected.map((e) => e.kind)).toEqual(["attachment", "attachment"]);
    expect(injected.map((e) => e.attachmentType)).toEqual([
      "agents_md",
      "environment_context",
    ]);
    expect(injected[0]?.loadedCategory).toBe("claude-md");
    const prompts = p.events.filter((e) => e.kind === "user-prompt");
    expect(prompts.map((e) => e.body)).toEqual(["Add a test."]);
  });

  test("inter-agent messages are kept", () => {
    const msg = codex().events.find(
      (e) => e.attachmentType === "agent_message"
    );
    expect(msg?.title).toBe("message from /root");
    expect(msg?.body).toBe("Check the parser.");
  });
});

describe("pi context entries", () => {
  test("the system message renders every named section and its tools", () => {
    const sys = pi().events.find((e) => e.kind === "system-prompt");
    expect(sys?.title).toBe("System prompt");
    expect(sys?.preview).toBe("preamble · project_context · cwd · tools (1)");
    expect(sys?.body).toContain("## preamble\n\nYou are pi, a coding agent.");
    expect(sys?.body).toContain("### read");
  });

  test("a later system message is an update and names removed sections", () => {
    const updates = pi().events.filter((e) => e.kind === "system-prompt");
    expect(updates.map((e) => e.title)).toEqual([
      "System prompt",
      "System prompt update",
      "System prompt update",
    ]);
    expect(updates[2]?.body).toContain('(section "cwd" removed)');
    expect(updates[2]?.body).toContain("Be brief.");
  });

  test("a compaction's new system message sits below the compaction", () => {
    const p = pi();
    const pos = p.events.findIndex((e) => e.kind === "compaction");
    const next = p.events[pos + 1];
    expect(next?.kind).toBe("system-prompt");
    expect(next?.index).toBe(p.events[pos]?.index ?? -1);
    expect(next?.body).toContain("Resume from the summary.");
  });

  test("session_info names the session", () => {
    expect(pi().title).toBe("read the readme");
  });

  test("string user content and images are kept", () => {
    const p = pi();
    expect(p.events.find((e) => e.body === "read the readme")?.kind).toBe(
      "user-prompt"
    );
    expect(p.events.find((e) => e.attachmentType === "image")?.body).toBe(
      "[image: image/png]"
    );
    const result = p.events.find((e) => e.kind === "tool-result");
    expect(result?.body).toBe("screenshot:[image: image/jpeg]");
  });

  test("custom messages and in-context bash runs are attachments", () => {
    const p = pi();
    const custom = p.events.find((e) => e.attachmentType === "custom_message");
    expect(custom?.title).toBe("todo-reminder");
    const bash = p.events.filter((e) => e.attachmentType === "bash_execution");
    expect(bash.map((e) => e.title)).toEqual(["$ ls"]);
    expect(bash[0]?.body).toContain("README.md");
  });

  test("compaction and branch summaries are wrapped as the model sees them", () => {
    const p = pi();
    const compaction = p.events.find((e) => e.kind === "compaction");
    expect(compaction?.body).toContain("User asked to read the readme.");
    expect(compaction?.body.endsWith("</summary>")).toBe(true);
    expect(p.compactionIndexes).toEqual([compaction?.index ?? -1]);
    expect(p.events.find((e) => e.kind === "summary")?.title).toBe(
      "Branch summary"
    );
  });

  test("a system prompt stays inside the measured system floor", () => {
    const p = pi();
    const a = analyze(p);
    const first = a.snapshots[0];
    expect(first?.slices.system_tools).toBe(a.systemOverheadTokens);
    expect(first?.slices.other).toBe(0);
    expect(a.systemOverheadTokens).toBe(
      analyze(withoutPrompt(p)).systemOverheadTokens
    );
  });
});

describe("claude context entries", () => {
  test("prompt_snapshot becomes a system prompt without the boundary marker", () => {
    const [first] = claude().events.filter((e) => e.kind === "system-prompt");
    expect(first?.title).toBe("System prompt");
    expect(first?.body).toBe(
      "You are an interactive agent.\n\n# Environment\n - cwd: /repo"
    );
    expect(first?.body).not.toContain("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__");
  });

  test("repeat snapshots add only what changed", () => {
    const prompts = claude().events.filter(
      (e) => e.kind === "system-prompt" && !e.isSidechain
    );
    expect(prompts.map((e) => e.title)).toEqual([
      "System prompt",
      "System prompt update",
    ]);
    expect(prompts[1]?.preview).toBe("cli prefix · tools (1)");
    expect(prompts[1]?.body).not.toContain("You are an interactive agent.");
  });

  test("subagent snapshots track their own prompt, apart from main", () => {
    const side = claude().events.filter(
      (e) => e.kind === "system-prompt" && e.isSidechain
    );
    expect(side.map((e) => e.title)).toEqual(["System prompt"]);
    expect(side[0]?.body).toContain("You are a search subagent.");
  });

  test("rendered text wins over the structured payload", () => {
    const e = claude().events[0];
    expect(e?.attachmentType).toBe("instructions");
    expect(e?.loadedCategory).toBe("claude-md");
    expect(e?.body).toContain("<system-reminder>");
    expect(e?.preview).toBe("Codebase and user instructions are shown below.");
  });

  test("nested memory is CLAUDE.md context", () => {
    const e = claude().events[1];
    expect(e?.title).toBe("nested_memory: src/CLAUDE.md");
    expect(e?.loadedCategory).toBe("claude-md");
    expect(e?.body).toBe("Keep files small.");
  });

  test("images show as markers, never as base64", () => {
    const p = claude();
    expect(p.events.find((e) => e.attachmentType === "image")?.body).toBe(
      "[image: image/png]"
    );
    const result = p.events.find((e) => e.kind === "tool-result");
    expect(result?.body).not.toContain("iVBOR");
    expect(result?.body).toContain("[image: image/png]");
  });

  test("the pre-turn prompt stays inside the system floor, not other", () => {
    const p = claude();
    const a = analyze(p);
    expect(a.snapshots[0]?.slices.system_tools).toBe(a.systemOverheadTokens);
    expect(a.snapshots[0]?.slices.other).toBe(0);
    expect(a.systemOverheadTokens).toBe(
      analyze(withoutPrompt(p)).systemOverheadTokens
    );
  });
});
