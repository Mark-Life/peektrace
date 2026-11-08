/** Owned root `--help` page. The lib renders every description column padded to
 * the widest usage line (about 235 columns here), so the root page is built by hand. */
import { HelpDoc, Span } from "@effect/cli";
import { DESCRIPTION } from "./description";
import {
  DOCTOR_SUMMARY,
  MEMORY_LS_SUMMARY,
  MEMORY_RM_SUMMARY,
  MEMORY_SHOW_SUMMARY,
  SERVE_SUMMARY,
  SESSIONS_ANALYZE_SUMMARY,
  SESSIONS_LS_SUMMARY,
  STATS_DRILL_SUMMARY,
  STATS_PROMPT_SUMMARY,
  STATS_REFRESH_SUMMARY,
  STATS_SUMMARY,
  STATS_USAGE_SUMMARY,
  STATUSLINE_SUMMARY,
  TUI_SUMMARY,
  UPGRADE_SUMMARY,
} from "./summaries";

/** Every runnable command, in display order. */
export const COMMAND_ROWS = [
  { path: "serve", summary: SERVE_SUMMARY },
  { path: "tui", summary: TUI_SUMMARY },
  { path: "sessions ls", summary: SESSIONS_LS_SUMMARY },
  { path: "sessions analyze", summary: SESSIONS_ANALYZE_SUMMARY },
  { path: "stats", summary: STATS_SUMMARY },
  { path: "stats drill", summary: STATS_DRILL_SUMMARY },
  { path: "stats prompt", summary: STATS_PROMPT_SUMMARY },
  { path: "stats usage", summary: STATS_USAGE_SUMMARY },
  { path: "stats refresh", summary: STATS_REFRESH_SUMMARY },
  { path: "memory ls", summary: MEMORY_LS_SUMMARY },
  { path: "memory show", summary: MEMORY_SHOW_SUMMARY },
  { path: "memory rm", summary: MEMORY_RM_SUMMARY },
  { path: "statusline", summary: STATUSLINE_SUMMARY },
  { path: "doctor", summary: DOCTOR_SUMMARY },
  { path: "upgrade", summary: UPGRADE_SUMMARY },
] as const;

/** Root flags: `usage` is what the page prints, `description` the option text
 * shared with completion scripts. */
export const ROOT_FLAGS = {
  json: {
    usage: "--json",
    description: "Emit raw JSON instead of rendered tables",
  },
  readOnly: {
    usage: "--read-only",
    description: "Refuse any mutating command (safe mode)",
  },
  pretty: {
    usage: "--pretty",
    description: "Render aligned tables, not tab-separated text",
  },
  remote: {
    usage: "--remote <url>",
    description: "Target a running `peektrace serve` over HTTP",
  },
  otel: {
    usage: "--otel",
    description: "Log Effect spans to stderr (or set PEEKTRACE_OTEL)",
  },
  telemetry: {
    usage: "--no-telemetry",
    description: "Local wide-event telemetry (on by default)",
    /** Help-page text; the shared description reads wrong next to `usage`. */
    page: "Disable local wide-event telemetry (on by default)",
  },
} as const;

/** Flags handled by the lib, listed by hand because built-ins are hidden. */
const BUILTIN_FLAGS = [
  { usage: "--log-level <level>", description: "Set the minimum log level" },
  { usage: "--completions <shell>", description: "Print a completion script" },
  { usage: "-v, --version", description: "Print the version" },
  { usage: "-h, --help", description: "Show help" },
] as const;

const EXAMPLES = [
  "peektrace serve",
  "peektrace sessions analyze --latest",
  "peektrace stats --days 7",
] as const;

const INDENT = "  ";

const pad = (rows: readonly (readonly [string, string])[]) => {
  const width = Math.max(...rows.map(([left]) => left.length));
  return rows
    .map(([left, right]) => `${INDENT}${left.padEnd(width)}  ${right}`)
    .join("\n");
};

const MAX_WIDTH = 80;

/** Word-wrap the description so no line passes 80 columns. */
const descriptionLines = () => {
  const lines: string[] = [];
  let line = "";
  for (const word of DESCRIPTION.split(" ")) {
    if (
      line !== "" &&
      INDENT.length + line.length + 1 + word.length > MAX_WIDTH
    ) {
      lines.push(line);
      line = word;
    } else {
      line = line === "" ? word : `${line} ${word}`;
    }
  }
  lines.push(line);
  return lines.map((text) => `${INDENT}${text}`).join("\n");
};

export const rootHelpDoc = (version: string) =>
  HelpDoc.blocks([
    HelpDoc.h1(`Peektrace ${version}`),
    HelpDoc.h1("USAGE"),
    HelpDoc.p(`${INDENT}peektrace [global flags] <command>`),
    HelpDoc.h1("DESCRIPTION"),
    HelpDoc.p(Span.text(descriptionLines())),
    HelpDoc.h1("COMMANDS"),
    HelpDoc.p(
      Span.text(pad(COMMAND_ROWS.map((row) => [row.path, row.summary])))
    ),
    HelpDoc.h1("GLOBAL OPTIONS"),
    HelpDoc.p(
      Span.text(
        pad([
          ...Object.values(ROOT_FLAGS).map(
            (flag) =>
              [
                flag.usage,
                "page" in flag ? flag.page : flag.description,
              ] as const
          ),
          ...BUILTIN_FLAGS.map(
            (flag) => [flag.usage, flag.description] as const
          ),
        ])
      )
    ),
    HelpDoc.h1("EXAMPLES"),
    HelpDoc.p(Span.text(EXAMPLES.map((line) => `${INDENT}${line}`).join("\n"))),
    HelpDoc.p("Run peektrace <command> --help for its flags."),
  ]);
