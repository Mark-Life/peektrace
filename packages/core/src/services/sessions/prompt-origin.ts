/** Who wrote a Claude Code `user` line.
 *
 * Every line the model reads as `role: user` is logged as `type: "user"`, so the
 * line type alone cannot tell a person's prompt from a task notification, a
 * `ScheduleWakeup` tick or a skill body the harness injected. Newer transcripts
 * say so outright (`origin.kind`, `promptSource`, `turnOrigin`); older ones only
 * through `isMeta` and the tags the harness wraps its own text in, so both are
 * read, explicit fields first.
 */
import type { PromptOrigin, TimelineEvent } from "./schema";

type Raw = Record<string, unknown>;

const TASK_NOTIFICATION = /^\s*<task-notification>/;

/** Tags Claude Code writes around a slash command or `!` shell line the user typed. */
const COMMAND = /^\s*<(?:command-name|command-message|bash-input)>/;

/** Tags around what a command printed: the harness's output, not the user's. */
const COMMAND_OUTPUT =
  /^\s*<(?:local-command-stdout|local-command-stderr|bash-stdout|bash-stderr)>/;

const COMMAND_NAME = /<command-name>\s*([^<]*?)\s*<\/command-name>/;
const COMMAND_ARGS = /<command-args>\s*([^<]*?)\s*<\/command-args>/;

/** One `<tag>value</tag>` field of a task notification. */
const tagValue = (text: string, tag: string) =>
  new RegExp(`<${tag}>\\s*([^<]*?)\\s*</${tag}>`).exec(text)?.[1];

const asObj = (v: unknown): Raw | undefined =>
  v && typeof v === "object" && !Array.isArray(v) ? (v as Raw) : undefined;

/** The origin of a user line from its fields, with the body as the fallback. */
export const claudePromptOrigin = (o: Raw, text: string): PromptOrigin => {
  const kind = asObj(o.origin)?.kind;
  const turnOrigin = o.turnOrigin;
  if (
    kind === "task-notification" ||
    turnOrigin === "task_notification" ||
    TASK_NOTIFICATION.test(text)
  ) {
    return "task-notification";
  }
  if (turnOrigin === "scheduled") {
    return "scheduled";
  }
  // Skill bodies the harness expands from a typed command carry the same
  // tags but are `isMeta`; a typed command never is.
  if (o.isMeta === true || COMMAND_OUTPUT.test(text)) {
    return "harness";
  }
  if (COMMAND.test(text)) {
    return "command";
  }
  if (o.promptSource === "sdk" || turnOrigin === "sdk") {
    return "sdk";
  }
  return "human";
};

/** Origins a person sent: they read on the user's side of the chat. */
const FROM_USER: ReadonlySet<PromptOrigin> = new Set([
  "human",
  "sdk",
  "command",
]);

/** Whether a prompt came from the user (typed, sent through the SDK, or a
 *  slash command) rather than from the harness. Unclassified prompts (agents
 *  whose transcripts carry no origin) count as the user's. */
export const isUserAuthored = (
  e: Pick<TimelineEvent, "kind" | "promptOrigin">
) =>
  e.kind === "user-prompt" &&
  (e.promptOrigin === undefined || FROM_USER.has(e.promptOrigin));

/** Row title and preview for a prompt, named after what wrote it. */
export const promptLabel = (
  origin: PromptOrigin,
  text: string
): { readonly preview?: string; readonly title: string } => {
  switch (origin) {
    case "task-notification": {
      const status = tagValue(text, "status");
      const summary = tagValue(text, "summary");
      return {
        title: status ? `Task notification (${status})` : "Task notification",
        ...(summary ? { preview: summary } : {}),
      };
    }
    case "scheduled":
      return { title: "Scheduled prompt" };
    case "harness":
      return {
        title: COMMAND_OUTPUT.test(text) ? "Command output" : "Harness message",
      };
    case "command": {
      const name = COMMAND_NAME.exec(text)?.[1];
      if (!name) {
        return { title: "Command" };
      }
      const args = COMMAND_ARGS.exec(text)?.[1];
      const line = args ? `${name} ${args}` : name;
      return { title: `Command ${name}`, preview: line };
    }
    default:
      return { title: "User prompt" };
  }
};
