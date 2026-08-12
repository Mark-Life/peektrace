/** Context injected between messages, as one centred cluster of chips.
 *
 * A real session drops five to twenty attachments at a turn boundary — CLAUDE.md,
 * skill listings, files pulled into the window. As rows they bury the
 * conversation; as chips they read as one line of "here is what was loaded", and
 * each chip still opens to its full recorded body.
 */
import type { LoadedCategory } from "@workspace/core/services/sessions/schema";
import {
  CodeBlock,
  CodeBlockCopyButton,
} from "@workspace/ui/components/ai-elements/code-block";
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import { fmtK } from "@workspace/viz/lib/session-format";
import {
  BellIcon,
  BotIcon,
  BrainIcon,
  ChevronRightIcon,
  FileIcon,
  FileTextIcon,
  InfoIcon,
  MonitorIcon,
  PaperclipIcon,
  PlugIcon,
  SparklesIcon,
  WrenchIcon,
} from "lucide-react";
import { useState } from "react";
import { chipLabel, type TranscriptRowRef } from "../../lib/chat-lane";
import { chatCollapseId } from "../../lib/session-view";
import { EMPTY_BODY, languageOfPath } from "../../lib/tool-event";

/** How many chips show before the rest fold behind a "+N more". */
const CHIP_LIMIT = 6;

/** Below this the token figure is noise; above it, it is the reason to look. */
const TOKEN_CHIP_FLOOR = 500;

const CATEGORY_ICONS = {
  agents: BotIcon,
  "claude-md": FileTextIcon,
  file: FileIcon,
  ide: MonitorIcon,
  mcp: PlugIcon,
  memory: BrainIcon,
  other: PaperclipIcon,
  reminder: BellIcon,
  skills: SparklesIcon,
  tools: WrenchIcon,
} as const satisfies Record<LoadedCategory, unknown>;

/** The icon for one chip: its budget category, or a paperclip when unclassified. */
export const chipIcon = (category?: LoadedCategory) =>
  category ? CATEGORY_ICONS[category] : PaperclipIcon;

const CHIP =
  "group/chip h-6 cursor-pointer gap-1.5 px-2 font-mono text-[11px] hover:bg-muted";

/** One cluster of adjacent context injections, with the opened bodies stacked
 *  below the whole row (never inline, which would break the wrap line). */
export const ChatContextRun = ({
  allOpen,
  forceExpand,
  isOpen,
  items,
  onToggle,
}: {
  readonly allOpen: boolean;
  /** A filter is active: never let the "+N more" fold hide a match. */
  readonly forceExpand: boolean;
  readonly isOpen: (id: string) => boolean;
  readonly items: readonly TranscriptRowRef[];
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  const [showAll, setShowAll] = useState(false);
  const expanded = showAll || allOpen || forceExpand;
  const shown = expanded ? items : items.slice(0, CHIP_LIMIT);
  const hidden = items.length - shown.length;
  const tokens = items.reduce((n, { e }) => n + e.tokensEst, 0);
  const opened = items.filter(({ pos }) => isOpen(chatCollapseId(pos)));

  return (
    <div className="flex w-full flex-col gap-1.5" data-testid="chat-chip-run">
      <p className="text-center text-[10px] text-muted-foreground">
        +{items.length} items loaded · ~{fmtK(tokens)} tok
      </p>
      <div className="flex flex-wrap items-center justify-center gap-1.5">
        {shown.map(({ e, pos }) => {
          const id = chatCollapseId(pos);
          const open = isOpen(id);
          const Icon =
            e.kind === "meta" ? InfoIcon : chipIcon(e.loadedCategory);
          return (
            <Badge asChild key={id} variant="outline">
              <button
                className={CHIP}
                data-kind={e.kind}
                data-lane="center"
                data-sidechain={e.isSidechain ? "true" : "false"}
                data-state={open ? "open" : "closed"}
                data-testid="chat-chip"
                id={id}
                onClick={() => onToggle(id, !open)}
                title={e.attachmentType ?? e.kind}
                type="button"
              >
                <Icon className="size-3 shrink-0" />
                <span className="max-w-56 truncate">{chipLabel(e)}</span>
                {e.tokensEst >= TOKEN_CHIP_FLOOR ? (
                  <span className="text-muted-foreground">
                    ~{fmtK(e.tokensEst)}
                  </span>
                ) : null}
                <ChevronRightIcon
                  className={cn(
                    "size-3 transition-transform",
                    open && "rotate-90"
                  )}
                />
              </button>
            </Badge>
          );
        })}
        {hidden > 0 ? (
          <Badge asChild variant="outline">
            <button
              className={CHIP}
              onClick={() => setShowAll(true)}
              type="button"
            >
              +{hidden} more
            </button>
          </Badge>
        ) : null}
      </div>
      {opened.map(({ e, pos }) => (
        <div className="flex w-full flex-col gap-1" key={chatCollapseId(pos)}>
          <p className="px-1 font-mono text-[10px] text-muted-foreground">
            {chipLabel(e)}
          </p>
          <CodeBlock
            className="max-h-[28rem] overflow-auto"
            code={e.body || EMPTY_BODY}
            language={e.kind === "meta" ? "text" : languageOfPath(chipLabel(e))}
          >
            <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
          </CodeBlock>
        </div>
      ))}
    </div>
  );
};
