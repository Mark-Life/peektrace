/** Context injected between messages, as one centred cluster of chips.
 *
 * A real session drops five to twenty attachments at a turn boundary — CLAUDE.md,
 * skill listings, files pulled into the window. As rows they bury the
 * conversation; as chips they read as one line of "here is what was loaded", and
 * each chip still opens to its full recorded body.
 */
import type { LoadedCategory } from "@workspace/core/services/sessions/schema";
import {
  eventTokens,
  tokenLabel,
} from "@workspace/core/services/sessions/tokens";
import { Badge } from "@workspace/ui/components/badge";
import { fmtK } from "@workspace/viz/lib/session-format";
import {
  BellIcon,
  BotIcon,
  BrainIcon,
  ChevronUpIcon,
  FileIcon,
  FileTextIcon,
  InfoIcon,
  MonitorIcon,
  PaperclipIcon,
  PlugIcon,
  SparklesIcon,
  WrenchIcon,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import {
  chipLabel,
  type RunItem,
  type TranscriptRowRef,
} from "../../lib/chat-lane";
import { chatCollapseId } from "../../lib/session-view";
import { ContextEditBadge, ContextEditNote } from "./context-edit-mark";
import { EventBody } from "./event-body";

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

/** The icon for one item of a run: info for harness lines, else its category. */
const itemIcon = (e: TranscriptRowRef["e"]) =>
  e.kind === "meta" || e.kind === "system"
    ? InfoIcon
    : chipIcon(e.loadedCategory);

const ContextChip = ({
  item: { e, pos },
  onToggle,
}: {
  readonly item: TranscriptRowRef;
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  const id = chatCollapseId(pos);
  const Icon = itemIcon(e);
  return (
    <Badge asChild variant="outline">
      <button
        aria-expanded={false}
        className={CHIP}
        data-kind={e.kind}
        data-lane="center"
        data-sidechain={e.isSidechain ? "true" : "false"}
        data-state="closed"
        data-testid="chat-chip"
        id={id}
        onClick={() => onToggle(id, true)}
        title={e.attachmentType ?? e.kind}
        type="button"
      >
        <Icon className="size-3 shrink-0" />
        <span className="max-w-56 truncate">{chipLabel(e)}</span>
        <ContextEditBadge className="h-4 px-1" e={e} />
        {eventTokens(e).tokens >= TOKEN_CHIP_FLOOR ? (
          <span className="text-muted-foreground">{tokenLabel(e, fmtK)}</span>
        ) : null}
      </button>
    </Badge>
  );
};

/** The card is the body's frame, so the body drops its own border. */
const FLAT_BODY =
  "max-h-[28rem] overflow-auto rounded-none border-0 bg-transparent";

/** An opened chip, grown in place into a card: the chip itself is its header,
 *  so what was clicked stays where it was and names the body under it. */
const ContextCard = ({
  item: { e, pos },
  onToggle,
}: {
  readonly item: TranscriptRowRef;
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  const id = chatCollapseId(pos);
  const Icon = itemIcon(e);
  return (
    <section
      className="w-full overflow-hidden rounded-md border border-primary/40 bg-background"
      data-testid="chat-chip-card"
    >
      <button
        aria-expanded
        className="flex w-full items-center gap-2 border-primary/30 border-b bg-primary/10 px-2.5 py-1.5 text-left text-xs hover:bg-primary/15"
        data-kind={e.kind}
        data-lane="center"
        data-sidechain={e.isSidechain ? "true" : "false"}
        data-state="open"
        data-testid="chat-chip"
        id={id}
        onClick={() => onToggle(id, false)}
        title="Collapse"
        type="button"
      >
        <Icon className="size-3.5 shrink-0 text-primary" />
        <span className="min-w-0 truncate font-medium font-mono">
          {chipLabel(e)}
        </span>
        {e.attachmentType ? (
          <Badge className="h-4 px-1 font-mono" variant="outline">
            {e.attachmentType}
          </Badge>
        ) : null}
        <ContextEditBadge className="h-4 px-1" e={e} />
        <span className="ml-auto flex shrink-0 items-center gap-1.5 font-mono text-[10px] text-muted-foreground">
          {tokenLabel(e, fmtK)} tok
          <ChevronUpIcon className="size-3.5" />
        </span>
      </button>
      <ContextEditNote e={e} />
      <EventBody className={FLAT_BODY} e={e} />
    </section>
  );
};

type Segment =
  | { readonly type: "chips"; readonly items: TranscriptRowRef[] }
  | { readonly type: "card"; readonly item: TranscriptRowRef };

/** Split the run at every opened item: closed neighbours stay chip rows above
 *  and below it, so the run keeps its order. */
export const segmentsOf = (
  items: readonly TranscriptRowRef[],
  open: (item: TranscriptRowRef) => boolean
): Segment[] => {
  const out: Segment[] = [];
  for (const item of items) {
    const last = out.at(-1);
    if (open(item)) {
      out.push({ type: "card", item });
    } else if (last?.type === "chips") {
      last.items.push(item);
    } else {
      out.push({ type: "chips", items: [item] });
    }
  }
  return out;
};

/** Hide chips that repeat an earlier one unchanged, unless something asks to
 *  see them: the user, "Expand all", an active filter, or an opened repeat.
 *  Hiding is render-only; `items` and their order are never changed. */
export const foldRepeats = ({
  allOpen,
  forceExpand,
  items,
  opened,
  showRepeats,
}: {
  readonly allOpen: boolean;
  readonly forceExpand: boolean;
  readonly items: readonly RunItem[];
  readonly opened: (item: RunItem) => boolean;
  readonly showRepeats: boolean;
}) => {
  const reveal =
    showRepeats ||
    allOpen ||
    forceExpand ||
    items.some((item) => item.repeat && opened(item));
  const visible = reveal ? items : items.filter((item) => !item.repeat);
  return { folded: items.length - visible.length, visible };
};

/** The chips on screen. One `showAll` flag lifts both the repeat fold and the
 *  `CHIP_LIMIT` cap, so a single click never swaps one hidden chip for another. */
export const chipWindow = ({
  showAll,
  ...args
}: Omit<Parameters<typeof foldRepeats>[0], "showRepeats"> & {
  readonly showAll: boolean;
}) => {
  const { folded, visible } = foldRepeats({ ...args, showRepeats: showAll });
  const lastOpened = visible.reduce(
    (last, item, i) => (args.opened(item) ? i : last),
    -1
  );
  const expanded =
    showAll || args.allOpen || args.forceExpand || lastOpened >= CHIP_LIMIT;
  const shown = expanded ? visible : visible.slice(0, CHIP_LIMIT);
  return { folded, hidden: visible.length - shown.length, shown };
};

const ChipRow = ({ children }: { readonly children: ReactNode }) => (
  <div className="flex flex-wrap items-center justify-center gap-1.5">
    {children}
  </div>
);

/** One cluster of adjacent context injections. Opening a chip grows it in
 *  place into a card; the chips before it wrap above and the ones after it
 *  below, so the order of what was loaded never changes. */
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
  readonly items: readonly RunItem[];
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  const [showAll, setShowAll] = useState(false);
  const opened = (item: TranscriptRowRef) => isOpen(chatCollapseId(item.pos));
  const { folded, hidden, shown } = chipWindow({
    allOpen,
    forceExpand,
    items,
    opened,
    showAll,
  });
  const tokens = items.reduce((n, { e }) => n + eventTokens(e).tokens, 0);
  const segments = segmentsOf(shown, opened);
  const extras =
    folded > 0 || hidden > 0 ? (
      <>
        {folded > 0 ? (
          <Badge asChild variant="outline">
            <button
              className={`${CHIP} text-muted-foreground`}
              data-testid="chat-chip-unchanged"
              onClick={() => setShowAll(true)}
              title="Same as earlier in this session"
              type="button"
            >
              {folded} unchanged
            </button>
          </Badge>
        ) : null}
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
      </>
    ) : null;

  return (
    <div className="flex w-full flex-col gap-1.5" data-testid="chat-chip-run">
      <p className="text-center text-[10px] text-muted-foreground">
        +{items.length} items loaded ·{" "}
        {items.every(({ e }) => eventTokens(e).measured) ? "" : "~"}
        {fmtK(tokens)} tok
      </p>
      {segments.map((seg, i) =>
        seg.type === "card" ? (
          <ContextCard item={seg.item} key={seg.item.pos} onToggle={onToggle} />
        ) : (
          <ChipRow key={seg.items[0]?.pos}>
            {seg.items.map((item) => (
              <ContextChip item={item} key={item.pos} onToggle={onToggle} />
            ))}
            {i === segments.length - 1 ? extras : null}
          </ChipRow>
        )
      )}
      {segments.at(-1)?.type !== "chips" && extras ? (
        <ChipRow>{extras}</ChipRow>
      ) : null}
    </div>
  );
};
