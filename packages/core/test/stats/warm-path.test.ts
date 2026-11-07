/** What a warm pass leans on: binaries lexed once at extract time. */
import { describe, expect, test } from "bun:test";
import { extractFacts } from "../../src/services/stats/extract";
import { isBashFact } from "../../src/services/stats/schema";
import { ranBins } from "../../src/services/stats/shell-lex";
import { parsedOf } from "./fixture-session";

describe("ran bins", () => {
  test("extract stores each binary the stored command runs, once", () => {
    const facts = extractFacts(
      parsedOf([
        {
          name: "Bash",
          input: {
            command: "FOO=1 bun run build && git status | head -5; bun test",
          },
          isError: false,
          output: "ok",
        },
      ])
    );
    const bash = facts.find(isBashFact);
    expect(bash?.ran).toEqual(["bun", "git", "head"]);
    expect(bash?.ran).toEqual(ranBins(bash?.cmd ?? ""));
  });
});
