/** Unit tests for the OpenCode dialect parser.
 *
 * Feeds a hand-written dialect fixture (session line + a user text message + an
 * assistant message with reasoning + a completed tool + an error tool + a
 * compaction) through the parser and asserts meta extraction, tool-call/result
 * pairing, error flagging, reasoning mapping, compaction indexing, per-turn
 * ground-truth usage, and the never-throw-on-garbage guarantee.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  buildOpencodeHeader,
  opencodeParser,
  parseOpencodeSession,
} from "../../src/services/sessions/parsers/opencode";

const FIXTURE = join(
  import.meta.dir,
  "../fixtures/sessions-opencode/session-fixture.jsonl"
);
const TEXT = readFileSync(FIXTURE, "utf8");
const SESSION_ID = "ses_test123";

const parse = () =>
  parseOpencodeSession({
    text: TEXT,
    path: `${FIXTURE}#${SESSION_ID}`,
    sessionId: "fallback-id",
    slug: "/Users/demo/proj",
  });

describe("parseOpencodeSession (meta)", () => {
  test("extracts provider + session metadata from the session line", () => {
    const p = parse();
    expect(p.provider).toBe("opencode");
    expect(p.sessionId).toBe(SESSION_ID);
    expect(p.cwd).toBe("/Users/demo/proj");
    expect(p.title).toBe("Test session");
    expect(p.version).toBe("1.18.2");
    expect(p.subagents).toEqual([]);
    expect(p.startedAt).toBe(new Date(1_784_148_745_485).toISOString());
    expect(p.endedAt).toBe(new Date(1_784_148_748_261).toISOString());
  });

  test("models dedup the session + message model ids", () => {
    expect(parse().models).toEqual(["gpt-5.6-terra-fast"]);
  });

  test("nativeContextWindow is left unset (analyze infers from the model)", () => {
    expect(parse().nativeContextWindow).toBeUndefined();
  });
});

describe("parseOpencodeSession (events)", () => {
  test("timeline drops step-start/step-finish control parts", () => {
    const kinds = parse().events.map((e) => e.kind);
    // step-start/step-finish produce no events; every other part does, in order.
    expect(kinds).toEqual([
      "user-prompt",
      "assistant-thinking",
      "tool-call",
      "tool-result",
      "tool-call",
      "tool-result",
      // a non-terminal (running) tool contributes a call but no synthetic result.
      "tool-call",
      "attachment",
      "attachment",
      "attachment",
      "system",
      "system",
      "system",
      "compaction",
    ]);
  });

  test("each event inherits its owning message's ts", () => {
    const p = parse();
    // user part carries no time of its own; it inherits msg_user1.time.created.
    expect(p.events[0].ts).toBe(new Date(1_784_148_745_485).toISOString());
    const thinking = p.events.find((e) => e.kind === "assistant-thinking");
    expect(thinking?.ts).toBe(new Date(1_784_148_745_500).toISOString());
  });

  test("a non-terminal tool part emits a call with no synthetic result", () => {
    const p = parse();
    const call = p.events.find(
      (e) => e.kind === "tool-call" && e.toolUseId === "call-run"
    );
    expect(call?.toolName).toBe("read");
    const result = p.events.find(
      (e) => e.kind === "tool-result" && e.toolUseId === "call-run"
    );
    expect(result).toBeUndefined();
  });

  test("user text becomes a user-prompt, reasoning becomes thinking", () => {
    const p = parse();
    expect(p.events[0].kind).toBe("user-prompt");
    expect(p.events[0].body).toBe("hey there, list the files");
    const thinking = p.events.find((e) => e.kind === "assistant-thinking");
    expect(thinking?.body).toBe("The user wants a file listing.");
  });

  test("a tool part fuses a tool-call + tool-result pair by callID", () => {
    const p = parse();
    const call = p.events.find(
      (e) => e.kind === "tool-call" && e.toolName === "glob"
    );
    const result = p.events.find(
      (e) => e.kind === "tool-result" && e.toolUseId === "call-abc"
    );
    expect(call?.toolUseId).toBe("call-abc");
    expect(call?.body).toContain("README*");
    expect(result?.body).toContain("README.md");
    expect(result?.isError).toBe(false);
  });

  test("an error tool result carries isError from state.status", () => {
    const p = parse();
    const result = p.events.find(
      (e) => e.kind === "tool-result" && e.toolUseId === "call-err"
    );
    expect(result?.isError).toBe(true);
    expect(result?.body).toBe("command failed");
  });

  test("compaction is recorded in compactionIndexes", () => {
    const p = parse();
    const compaction = p.events.find((e) => e.kind === "compaction");
    expect(compaction).toBeDefined();
    expect(p.compactionIndexes).toEqual([compaction?.index]);
  });

  test("newer part types map to sensible kinds", () => {
    const p = parse();
    const file = p.events.find((e) => e.attachmentType === "file");
    expect(file?.kind).toBe("attachment");
    expect(p.events.find((e) => e.attachmentType === "patch")?.kind).toBe(
      "attachment"
    );
    expect(p.events.find((e) => e.attachmentType === "snapshot")?.kind).toBe(
      "attachment"
    );
    // subtask / agent / retry are surfaced as system events, not dropped.
    const systemTitles = p.events
      .filter((e) => e.kind === "system")
      .map((e) => e.title);
    expect(systemTitles).toEqual(
      expect.arrayContaining(["subagent spawn", "agent", "retry"])
    );
  });
});

describe("parseOpencodeSession (turns)", () => {
  test("one turn per assistant message with ground-truth usage", () => {
    const p = parse();
    expect(p.turns).toHaveLength(1);
    const t = p.turns[0];
    expect(t.requestId).toBe("msg_asst1");
    expect(t.model).toBe("gpt-5.6-terra-fast");
    expect(t.inputTokens).toBe(8305);
    expect(t.cacheReadTokens).toBe(8192);
    expect(t.cacheCreationTokens).toBe(128);
    expect(t.outputTokens).toBe(6);
    // contextTokens includes cache WRITE, not just input + cache read.
    expect(t.contextTokens).toBe(8305 + 8192 + 128);
  });
});

describe("buildOpencodeHeader", () => {
  test("derives project from the directory basename, stamps opencode", () => {
    const h = buildOpencodeHeader({
      text: TEXT,
      id: SESSION_ID,
      slug: "/Users/demo/proj",
      path: `${FIXTURE}#${SESSION_ID}`,
      sizeBytes: Buffer.byteLength(TEXT, "utf8"),
      mtimeMs: 1_784_148_748_261,
    });
    expect(h.agent).toBe("opencode");
    expect(h.project).toBe("proj");
    expect(h.cwd).toBe("/Users/demo/proj");
    expect(h.model).toBe("gpt-5.6-terra-fast");
    expect(h.title).toBe("Test session");
    expect(h.messageCount).toBe(2);
    expect(h.sizeBytes).toBeGreaterThan(0);
    expect(h.startedAt).toBe(new Date(1_784_148_745_485).toISOString());
  });
});

describe("opencodeParser (defensive)", () => {
  test("exposes the opencode agent id", () => {
    expect(opencodeParser.agent).toBe("opencode");
  });

  test("never throws on a garbage / empty transcript", () => {
    const p = parseOpencodeSession({
      text: "{\nnotjson",
      path: "/tmp/broken#x",
      sessionId: "broken",
      slug: "",
    });
    expect(p.provider).toBe("opencode");
    expect(p.sessionId).toBe("broken");
    expect(p.events).toEqual([]);
    expect(p.turns).toEqual([]);
    expect(p.compactionIndexes).toEqual([]);

    const empty = parseOpencodeSession({
      text: "",
      path: "/tmp/empty#x",
      sessionId: "empty",
      slug: "",
    });
    expect(empty.events).toEqual([]);
    expect(empty.turns).toEqual([]);
  });
});
