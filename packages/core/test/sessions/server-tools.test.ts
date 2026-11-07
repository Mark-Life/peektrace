/** Server tools the API ran inside an assistant message.
 *
 * Web search and web fetch run on Anthropic's side: the call and its result
 * both arrive in the assistant message, and usage counts them under
 * `server_tool_use`. Unparsed, the call read as "no result" forever.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-claude/server-tools.jsonl"
);
const parsed = () =>
  parseClaudeSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "s1",
  });

describe("Claude server tools", () => {
  test("search hits and fetch errors become results linked to their call", () => {
    const results = parsed().events.filter((e) => e.kind === "tool-result");
    expect(
      results.map((e) => [e.title, e.toolUseId, e.isError ?? false])
    ).toEqual([
      ["web_search result", "srv_1", false],
      ["web_fetch result (error)", "srv_2", true],
    ]);
    expect(results[0]?.body).toBe(
      "- [Bun 1.4](https://bun.sh/blog/bun-v1.4) · 3 days ago"
    );
    expect(results[1]?.body).toBe("error: url_not_accessible");
  });

  test("usage counts land on the turn and sum on the session", () => {
    const p = parsed();
    expect(p.turns[0]?.webSearchRequests).toBe(1);
    expect(p.turns[0]?.webFetchRequests).toBe(1);
    const a = analyze(p);
    expect(a.webSearchRequests).toBe(1);
    expect(a.webFetchRequests).toBe(1);
  });
});
