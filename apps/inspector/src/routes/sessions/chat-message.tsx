/** Bubble primitives for the chat reading of a transcript.
 *
 * App-local on purpose: there is exactly one consumer, and every prop here
 * (collapse ids, token estimates, the raw-bytes escape hatch) is specific to a
 * recorded transcript rather than to a chat UI in general. Everything rendered
 * comes from `TimelineEvent.body`/`preview`/`title`, which the server redacts,
 * so no path here reaches around the redaction toggle.
 */
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import {
  CodeBlock,
  CodeBlockCopyButton,
} from "@workspace/ui/components/ai-elements/code-block";
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import { cn } from "@workspace/ui/lib/utils";
import {
  fmt,
  fmtK,
  fmtPct,
  ZONE_CLASSES,
  zoneOf,
} from "@workspace/viz/lib/session-format";
import { BrainIcon, ChevronRightIcon } from "lucide-react";
import { memo, type ReactNode, useState } from "react";
import { type ChatLane, needsClamp } from "../../lib/chat-lane";
import { EMPTY_BODY } from "../../lib/tool-event";
import { sameChatRow } from "../../lib/transcript-row";

/** Thinking content is a signature only — the tokens are real, the text is not. */
const NO_THINKING =
  "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band.";

const LANE_JUSTIFY: Record<ChatLane, string> = {
  agent: "justify-start",
  center: "justify-center",
  user: "justify-end",
};

/** How wide a column may grow before it wraps; prompts stay narrower than work. */
const LANE_WIDTH: Record<Exclude<ChatLane, "center">, string> = {
  agent: "max-w-[92%] sm:max-w-[85%]",
  user: "max-w-[85%] sm:max-w-[75%]",
};

const LANE_SURFACE: Record<Exclude<ChatLane, "center">, string> = {
  agent: "rounded-bl-sm border-border bg-muted/40",
  user: "rounded-br-sm border-primary/25 bg-primary/10",
};

const BUBBLE =
  "rounded-2xl border px-3.5 py-2.5 text-sm leading-relaxed wrap-break-word whitespace-pre-wrap";

/** Clamped body: a fixed height behind a fade, rather than a line count — one
 *  unwrapped 800-character line would otherwise clamp to nothing. */
const CLAMPED =
  "max-h-48 overflow-hidden [mask-image:linear-gradient(to_bottom,black_65%,transparent)]";

const OPENED = "max-h-[28rem] overflow-auto";

/** Height treatment for a body: nothing when it is short enough to just show. */
const clampClass = ({
  clamps,
  open,
}: {
  readonly clamps: boolean;
  readonly open: boolean;
}) => {
  if (!clamps) {
    return "";
  }
  return open ? OPENED : CLAMPED;
};

const META =
  "flex items-center gap-1.5 px-1 pb-0.5 font-mono text-[10px] text-muted-foreground";

/** Secondary actions stay out of the way until the message is hovered or focused. */
const REVEAL =
  "opacity-0 transition-opacity group-focus-within/msg:opacity-100 group-hover/msg:opacity-100";

/** One row of the conversation: which side it sits on, and whether it belongs to
 *  a subagent (marked with a rail rather than moved out of its lane). */
export const ChatRow = ({
  children,
  lane,
  sidechain,
}: {
  readonly children: ReactNode;
  readonly lane: ChatLane;
  readonly sidechain?: boolean;
}) => (
  <div
    className={cn(
      "group/msg flex w-full",
      LANE_JUSTIFY[lane],
      sidechain === true &&
        "border-muted-foreground/25 border-l-2 border-dashed pl-3"
    )}
  >
    {children}
  </div>
);

/** A control small enough to sit in the meta line without giving it height. */
const META_BUTTON = "h-4 px-1 text-[10px]";

/** The raw-bytes switch on its own, for callers that place it outside the flow
 *  of the thing it reveals — an `opacity-0` button still occupies its box, and
 *  inside a bubble that reads as broken padding under every one-line message. */
const RawButton = ({
  onClick,
  raw,
}: {
  readonly onClick: () => void;
  readonly raw: boolean;
}) => (
  <Button
    className={cn(META_BUTTON, !raw && REVEAL)}
    data-testid="chat-raw-toggle"
    onClick={onClick}
    size="xs"
    variant="ghost"
  >
    {raw ? "Hide raw" : "Raw"}
  </Button>
);

/** The exact recorded bytes, one click away from anything the layout reshapes.
 *  Only for surfaces that mount it when already open — see `RawButton`. */
export const RawToggle = ({ body }: { readonly body: string }) => {
  const [raw, setRaw] = useState(false);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex">
        <RawButton onClick={() => setRaw(!raw)} raw={raw} />
      </div>
      {raw ? (
        <CodeBlock
          className="max-h-[28rem] overflow-auto"
          code={body || EMPTY_BODY}
          language="text"
        >
          <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
        </CodeBlock>
      ) : null}
    </div>
  );
};

interface ChatBubbleProps {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly lane: Exclude<ChatLane, "center">;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
}

/** A prompt or a block of assistant text, in full — a message is the thing you
 *  came to read, so it renders its whole body, not the truncated preview. Long
 *  bodies clamp behind a fade until opened. Markdown stays literal: no renderer
 *  exists in this tree. */
const ChatBubbleBody = ({
  collapseId,
  e,
  lane,
  onToggle,
  open,
}: ChatBubbleProps) => {
  const hasBody = e.body.trim().length > 0;
  const clamps = needsClamp(e);
  const [raw, setRaw] = useState(false);
  return (
    <ChatRow lane={lane} sidechain={e.isSidechain === true}>
      <div
        className={cn("flex flex-col", LANE_WIDTH[lane])}
        data-kind={e.kind}
        data-lane={lane}
        data-sidechain={e.isSidechain ? "true" : "false"}
        data-testid="history-event"
        id={collapseId}
      >
        <div className={cn(META, lane === "user" && "justify-end")}>
          <span>{e.kind === "user-prompt" ? "you" : "assistant"}</span>
          {e.tokensEst ? <span>· ~{fmt(e.tokensEst)} tok</span> : null}
          {e.isSidechain ? (
            <Badge className="h-4" variant="secondary">
              sidechain
            </Badge>
          ) : null}
          {clamps && !raw ? (
            <Button
              className={META_BUTTON}
              data-testid="chat-show-all"
              onClick={() => onToggle(collapseId, !open)}
              size="xs"
              variant="ghost"
            >
              {open ? "Show less" : "Show all"}
            </Button>
          ) : null}
          <RawButton onClick={() => setRaw(!raw)} raw={raw} />
        </div>
        {raw ? (
          <CodeBlock
            className="max-h-[28rem] overflow-auto rounded-2xl"
            code={e.body || EMPTY_BODY}
            language="text"
          >
            <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
          </CodeBlock>
        ) : (
          <div className={cn(BUBBLE, LANE_SURFACE[lane])}>
            <div className={clampClass({ clamps, open })}>
              {hasBody ? (
                e.body
              ) : (
                <span className="text-muted-foreground italic">
                  {EMPTY_BODY}
                </span>
              )}
            </div>
          </div>
        )}
      </div>
    </ChatRow>
  );
};

export const ChatBubble = memo(ChatBubbleBody, sameChatRow);

/** Thinking, kept as a quiet strip: it carries real token cost but usually no
 *  text, so it never earns the width of a bubble. */
const ChatThinkingBody = ({
  collapseId,
  e,
  lane,
  onToggle,
  open,
}: ChatBubbleProps) => (
  <ChatRow lane={lane} sidechain={e.isSidechain === true}>
    <div
      className={cn("flex flex-col gap-1", LANE_WIDTH[lane])}
      data-kind={e.kind}
      data-lane={lane}
      data-sidechain={e.isSidechain ? "true" : "false"}
      data-testid="history-event"
      id={collapseId}
    >
      <button
        className="flex items-center gap-1.5 self-start rounded-2xl rounded-bl-sm border border-border border-dashed px-3 py-1.5 text-muted-foreground text-xs italic hover:bg-muted/30"
        data-state={open ? "open" : "closed"}
        onClick={() => onToggle(collapseId, !open)}
        type="button"
      >
        <ChevronRightIcon
          className={cn("size-3 transition-transform", open && "rotate-90")}
        />
        <BrainIcon className="size-3" />
        thinking{e.tokensEst ? ` · ~${fmtK(e.tokensEst)}` : ""}
      </button>
      {open ? (
        <div className="flex flex-col gap-1">
          <div
            className={cn(
              "wrap-break-word whitespace-pre-wrap rounded-md bg-muted/30 px-3 py-2 text-muted-foreground text-xs",
              OPENED
            )}
          >
            {e.body.trim().length > 0 ? e.body : NO_THINKING}
          </div>
          <RawToggle body={e.body} />
        </div>
      ) : null}
    </div>
  </ChatRow>
);

export const ChatThinking = memo(ChatThinkingBody, sameChatRow);

const BAND_TONE = {
  danger:
    "bg-red-500/15 px-3 py-1.5 text-center font-medium text-red-300 text-xs",
  muted:
    "max-w-2xl rounded-md border border-border border-dashed bg-muted/30 px-3 py-2 text-center text-xs",
  warn: "rounded-md border-amber-500/40 border-y bg-amber-500/10 px-3 py-1.5 text-center text-xs",
} as const;

/** A full-width notice across the conversation: a compaction, a rolling summary,
 *  or the dumb-zone crossing. */
export const ChatBand = ({
  children,
  tone,
}: {
  readonly children: ReactNode;
  readonly tone: keyof typeof BAND_TONE;
}) => (
  <div className="flex w-full justify-center">
    <div className={cn("w-full", BAND_TONE[tone])}>{children}</div>
  </div>
);

/** The rule between turns: what the model was, and how full its window already
 *  was — the context the table's `t{n}` gutter could only hint at. */
export const ChatTurnRule = ({
  ctxFrac,
  dumbZone,
  model,
  tokens,
  turn,
}: {
  readonly ctxFrac: number;
  readonly dumbZone: number;
  readonly model: string;
  readonly tokens: number;
  readonly turn: number;
}) => (
  <div
    className="flex items-center gap-2 py-1 text-[11px] text-muted-foreground"
    data-testid="chat-turn-rule"
  >
    <div className="h-px flex-1 bg-border" />
    <span>
      turn {turn} · {model} · ctx {fmtK(tokens)}{" "}
      <span className={ZONE_CLASSES[zoneOf({ dumbZone, frac: ctxFrac })].text}>
        ({fmtPct(ctxFrac)})
      </span>
    </span>
    <div className="h-px flex-1 bg-border" />
  </div>
);
