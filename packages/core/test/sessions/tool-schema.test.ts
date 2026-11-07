/** Tool definitions read as Markdown, not as a JSON dump. */
import { describe, expect, test } from "bun:test";
import {
  renderTool,
  toolWireText,
} from "../../src/services/sessions/tool-schema";

/** The shape Claude Code logs in a `prompt_snapshot`. */
const bash = {
  name: "Bash",
  description: "Executes a bash command.",
  schema: {
    name: "Bash",
    description: "Executes a bash command.",
    input_schema: {
      $schema: "https://json-schema.org/draft/2020-12/schema",
      type: "object",
      properties: {
        command: { type: "string", description: "The command to run" },
        timeout: { type: "number", maximum: 600_000 },
        mode: { enum: ["fg", "bg"], description: "Where it runs\nand why" },
        files: {
          type: "array",
          items: {
            type: "object",
            properties: { path: { type: "string" } },
            required: ["path"],
          },
        },
      },
      required: ["command"],
    },
  },
};

describe("renderTool", () => {
  test("a Claude Code tool shows once, with a parameter list", () => {
    expect(renderTool(bash)).toBe(
      [
        "### Bash",
        "Executes a bash command.",
        [
          "**Parameters**",
          "",
          "- `command` *string, required* — The command to run",
          "- `timeout` *number, maximum: 600000*",
          '- `mode` *"fg" | "bg"* — Where it runs\n  and why',
          "- `files` *object[]*",
          "  - `path` *string, required*",
        ].join("\n"),
      ].join("\n\n")
    );
  });

  test("a namespace lists its member tools one level down", () => {
    const text = renderTool({
      type: "namespace",
      name: "app",
      description: "App tools",
      tools: [{ name: "open_panel", description: "Open a panel" }],
    });
    expect(text).toBe(
      "### app\n\nApp tools\n\n#### open_panel\n\nOpen a panel"
    );
  });

  test("keys the layout does not know stay visible as JSON", () => {
    expect(
      renderTool({ name: "x", cache_control: { type: "ephemeral" } })
    ).toBe(
      '### x\n\n```json\n{\n  "cache_control": {\n    "type": "ephemeral"\n  }\n}\n```'
    );
  });

  test("the wire text is the API copy, without the duplicate description", () => {
    expect(JSON.parse(toolWireText(bash))).toEqual(bash.schema);
  });
});
