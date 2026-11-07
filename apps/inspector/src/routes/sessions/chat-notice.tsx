/** A prompt the harness wrote, centred between the two sides of the chat.
 *
 * Claude Code logs task notifications, `ScheduleWakeup` ticks and injected skill
 * bodies as `user` lines, so they used to read as the person talking. They are
 * the system talking: a quiet centred card that names what wrote it, folded to
 * its one-line summary until opened.
 */
import type {
  PromptOrigin,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import { tokenLabel } from "@workspace/core/services/sessions/tokens";
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import { fmtK } from "@workspace/viz/lib/session-format";
import {
  AlarmClockIcon,
  BellIcon,
  ChevronRightIcon,
  CogIcon,
  type LucideIcon,
  TerminalIcon,
} from "lucide-react";
import { memo } from "react";
import { sameChatRow } from "../../lib/transcript-row";
import { RawToggle } from "./chat-message";
import { ContextEditBadge, ContextEditNote } from "./context-edit-mark";
import { EventBody } from "./event-body";

const ORIGIN_ICON: Record<PromptOrigin, LucideIcon> = {
  command: TerminalIcon,
  harness: CogIcon,
  human: CogIcon,
  scheduled: AlarmClockIcon,
  sdk: CogIcon,
  "task-notification": BellIcon,
};

/** Every leaf `<tag>value</tag>` of a task notification, in order. */
const LEAF_TAG = /<([a-z][\w-]*)>([^<]*)<\/\1>/g;

/** The fields of a task notification, or an empty list when it has none. */
export const notificationFields = (body: string) =>
  [...body.matchAll(LEAF_TAG)]
    .map((m) => ({ at: m.index, tag: m[1] ?? "", value: (m[2] ?? "").trim() }))
    .filter((f) => f.value !== "");

/** A task notification is tagged fields, not prose: show them as a table. */
const NoticeBody = ({ e }: { readonly e: TimelineEvent }) => {
  const fields =
    e.promptOrigin === "task-notification" ? notificationFields(e.body) : [];
  if (fields.length === 0) {
    return <EventBody className="max-h-[28rem] overflow-auto" e={e} />;
  }
  return (
    <dl className="grid max-h-[28rem] grid-cols-[max-content_1fr] gap-x-3 gap-y-1 overflow-auto rounded-md border bg-background px-3 py-2 font-mono text-[11px]">
      {fields.map(({ at, tag, value }) => (
        <div className="contents" key={at}>
          <dt className="text-muted-foreground">{tag.replaceAll("_", " ")}</dt>
          <dd className="min-w-0 whitespace-pre-wrap break-words">{value}</dd>
        </div>
      ))}
    </dl>
  );
};

const ChatNoticeBody = ({
  collapseId,
  e,
  onToggle,
  open,
}: {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
}) => {
  const Icon = ORIGIN_ICON[e.promptOrigin ?? "harness"];
  return (
    <div className="flex w-full justify-center">
      <div
        className="w-full max-w-2xl rounded-md border border-border border-dashed bg-muted/30 px-3 py-2 text-xs"
        data-kind={e.kind}
        data-lane="center"
        data-origin={e.promptOrigin ?? ""}
        data-sidechain={e.isSidechain ? "true" : "false"}
        data-testid="history-event"
        id={collapseId}
      >
        <button
          className="flex w-full min-w-0 items-center gap-1.5 text-left"
          data-state={open ? "open" : "closed"}
          onClick={() => onToggle(collapseId, !open)}
          type="button"
        >
          <ChevronRightIcon
            className={cn(
              "size-3 shrink-0 text-muted-foreground transition-transform",
              open && "rotate-90"
            )}
          />
          <Icon className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="shrink-0 font-medium">{e.title}</span>
          <span className="min-w-0 truncate text-muted-foreground">
            {e.preview}
          </span>
          <span className="ml-auto flex shrink-0 items-center gap-1.5 font-mono text-[10px] text-muted-foreground">
            {e.promptSource ? (
              <Badge
                className="h-4 px-1"
                title="Claude Code promptSource"
                variant="outline"
              >
                {e.promptSource}
              </Badge>
            ) : null}
            <ContextEditBadge className="h-4" e={e} />
            {e.tokensEst ? `${tokenLabel(e, fmtK)} tok` : null}
          </span>
        </button>
        {open ? (
          <div className="mt-2 flex flex-col gap-1">
            <ContextEditNote e={e} />
            <NoticeBody e={e} />
            <RawToggle body={e.body} />
          </div>
        ) : null}
      </div>
    </div>
  );
};

export const ChatNotice = memo(ChatNoticeBody, sameChatRow);
