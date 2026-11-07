/** Tool definitions as readable Markdown.
 *
 * A tool reaches the model as a name, a description and a JSON Schema for its
 * input. Dumped as JSON inside Markdown, the schema loses its formatting and
 * the description shows twice (Claude Code logs it beside the API-shaped
 * `schema` copy). Instead each tool reads as a heading, its description, and
 * one list item per parameter: name, type, whether it is required, its own
 * description, and nested parameters indented under it.
 */

type Raw = Record<string, unknown>;

const asObj = (v: unknown): Raw | undefined =>
  v && typeof v === "object" && !Array.isArray(v) ? (v as Raw) : undefined;

/** Schema keys already shown by the list item itself. */
const SHOWN = new Set([
  "type",
  "description",
  "properties",
  "items",
  "required",
  "enum",
  "$schema",
  "additionalProperties",
  "title",
]);

const INDENT = "  ";

/** The input schema, wherever this agent put it. */
const inputSchema = (tool: Raw): Raw | undefined => {
  const wrapped = asObj(tool.schema);
  return (
    asObj(tool.input_schema) ??
    asObj(tool.inputSchema) ??
    asObj(tool.parameters) ??
    asObj(wrapped?.input_schema) ??
    asObj(wrapped?.parameters)
  );
};

/** A short type label: `string`, `"a" | "b"`, `string[]`, `object`. */
const typeLabel = (s: Raw): string => {
  if (Array.isArray(s.enum)) {
    return s.enum.map((v) => JSON.stringify(v)).join(" | ");
  }
  if (Array.isArray(s.type)) {
    return s.type.join(" | ");
  }
  if (s.type === "array") {
    const items = asObj(s.items);
    return items ? `${typeLabel(items)}[]` : "array";
  }
  if (typeof s.type === "string") {
    return s.type;
  }
  if (Array.isArray(s.anyOf ?? s.oneOf)) {
    return "union";
  }
  return "any";
};

/** Constraints the label does not carry (`minLength`, `default`, …). */
const constraints = (s: Raw) =>
  Object.entries(s)
    .filter(([k]) => !SHOWN.has(k))
    .map(([k, v]) => `${k}: ${JSON.stringify(v)}`);

/** Indent every line after the first so a multi-line text stays in its item. */
const hang = (text: string, depth: number) =>
  text.replaceAll("\n", `\n${INDENT.repeat(depth + 1)}`);

/** The properties nested under a parameter: an object's, or an array item's. */
const nestedOf = (s: Raw) => {
  if (asObj(s.properties)) {
    return s;
  }
  const items = asObj(s.items);
  return items && asObj(items.properties) ? items : undefined;
};

/** One list item per property of `schema`, nested ones indented beneath. */
const paramLines = (schema: Raw, depth: number): string[] => {
  const props = asObj(schema.properties) ?? {};
  const required = new Set(
    Array.isArray(schema.required) ? (schema.required as string[]) : []
  );
  return Object.entries(props).flatMap(([name, raw]) => {
    const s = asObj(raw) ?? {};
    const facts = [
      typeLabel(s),
      ...(required.has(name) ? ["required"] : []),
      ...constraints(s),
    ].join(", ");
    const desc =
      typeof s.description === "string" && s.description.trim() !== ""
        ? ` — ${hang(s.description.trim(), depth)}`
        : "";
    const nested = nestedOf(s);
    return [
      `${INDENT.repeat(depth)}- \`${name}\` *${facts}*${desc}`,
      ...(nested ? paramLines(nested, depth + 1) : []),
    ];
  });
};

/** Tool keys the Markdown already shows; anything else is kept as JSON. */
const TOOL_KEYS = new Set([
  "name",
  "description",
  "schema",
  "input_schema",
  "inputSchema",
  "parameters",
  "tools",
  "type",
  "eager_input_streaming",
]);

const HEADING_LEVEL = 3;

/**
 * One tool definition as Markdown: heading, description, parameter list. A
 * namespace (Codex groups app tools) lists its member tools one level down;
 * keys this layout does not know stay visible as a JSON block.
 */
export const renderTool = (tool: unknown, level = HEADING_LEVEL): string => {
  const o = asObj(tool) ?? {};
  const wrapped = asObj(o.schema);
  const name = o.name ?? wrapped?.name;
  const description = o.description ?? wrapped?.description;
  const schema = inputSchema(o);
  const params = schema ? paramLines(schema, 0) : [];
  const members = Array.isArray(o.tools) ? o.tools : [];
  const extra = Object.fromEntries(
    Object.entries(o).filter(([k]) => !TOOL_KEYS.has(k))
  );
  return [
    `${"#".repeat(level)} ${typeof name === "string" ? name : "tool"}`,
    typeof description === "string" ? description.trim() : "",
    params.length > 0 ? `**Parameters**\n\n${params.join("\n")}` : "",
    ...members.map((m) => renderTool(m, level + 1)),
    Object.keys(extra).length > 0
      ? `\`\`\`json\n${JSON.stringify(extra, null, 2)}\n\`\`\``
      : "",
  ]
    .filter(Boolean)
    .join("\n\n");
};

/** The definition as the model receives it: Claude Code logs the API-shaped
 *  copy under `schema` next to a duplicate name and description. */
export const toolWireText = (tool: unknown) =>
  JSON.stringify(asObj(tool)?.schema ?? tool);
