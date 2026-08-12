"use client";

/** Tool call / tool result presentation, vendored from Vercel's AI Elements.
 *
 * Upstream: https://elements.ai-sdk.dev/api/registry/tool.json. It diverges here
 * in the ways a *finished transcript* forces, since we render `AnalyzedSession`
 * events rather than a live AI SDK stream:
 *
 *  - `ToolState` replaces `ToolUIPart["state"]`. A recorded call is never
 *    "streaming" or "awaiting approval" — it either got a result, got a failed
 *    result, or never got one at all.
 *  - `ToolInput`/`ToolOutput` take `code` + `language` instead of an `unknown`
 *    payload: the shared highlighter needs an explicit language, and callers
 *    already unwrap JSON-encoded tool payloads into their real shape.
 *  - `ToolHeader` takes `icon`, `lead` and `children` slots, so a dense forensics
 *    list can hang turn numbers, badges and token counts off the same trigger row.
 *  - No `mb-4` on the root and no padding on `ToolContent`: spacing belongs to
 *    whatever stacks these, and each pane owns its own, so a payload can run
 *    edge to edge under its label at any density.
 */

import { Badge } from "@workspace/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import type { DiffRow, DiffRowKind, LineDiff } from "@workspace/core/diff";
import type {
  CodeBlockLanguage,
  CodeLine,
} from "@workspace/ui/lib/highlighter";
import { cn } from "@workspace/ui/lib/utils";
import {
  ChevronRightIcon,
  CircleDashedIcon,
  WrenchIcon,
  XCircleIcon,
} from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { useMemo } from "react";
import {
  CodeBlock,
  CodeBlockCopyButton,
  CODE_SURFACE,
  CodeTokens,
  highlightCode,
} from "./code-block";

/** What a recorded tool call ended up doing. */
export type ToolState = "completed" | "error" | "unanswered";

export type ToolProps = ComponentProps<typeof Collapsible>;

export const Tool = ({ className, ...props }: ToolProps) => (
  <Collapsible
    className={cn("group not-prose w-full rounded-md border", className)}
    {...props}
  />
);

/** Only the outcomes that are worth a badge — see `ToolStatusBadge`. */
type NotableState = Exclude<ToolState, "completed">;

const STATUS_LABELS: Record<NotableState, string> = {
  error: "error",
  unanswered: "no result",
};

const STATUS_ICONS: Record<NotableState, ReactNode> = {
  error: <XCircleIcon className="size-3" />,
  unanswered: <CircleDashedIcon className="size-3" />,
};

export type ToolStatusBadgeProps = Omit<
  ComponentProps<typeof Badge>,
  "children"
> & {
  readonly state: ToolState;
};

/** Icon + label for a call's outcome; errors take the destructive variant.
 *  A completed call renders nothing: it is the overwhelming majority of a
 *  transcript, so the badge marks every row without distinguishing any. Only
 *  the outcomes worth stopping on — an error, a call that never came back —
 *  earn the space. */
export const ToolStatusBadge = ({
  className,
  state,
  ...props
}: ToolStatusBadgeProps) =>
  state === "completed" ? null : (
    <Badge
      className={cn("shrink-0 gap-1 rounded-full", className)}
      variant={state === "error" ? "destructive" : "secondary"}
      {...props}
    >
      {STATUS_ICONS[state]}
      {STATUS_LABELS[state]}
    </Badge>
  );

export type ToolHeaderProps = ComponentProps<typeof CollapsibleTrigger> & {
  /** Replaces the wrench, e.g. to tell an invocation from what it returned. */
  readonly icon?: ReactNode;
  /** Rendered before the icon, e.g. a turn-number gutter. */
  readonly lead?: ReactNode;
  readonly name: string;
  readonly state: ToolState;
};

export const ToolHeader = ({
  children,
  className,
  icon,
  lead,
  name,
  state,
  ...props
}: ToolHeaderProps) => (
  <CollapsibleTrigger
    className={cn("flex w-full items-center gap-2 p-3 text-left", className)}
    {...props}
  >
    <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-90" />
    {lead}
    {icon ?? <WrenchIcon className="size-3.5 shrink-0 text-muted-foreground" />}
    <span className="shrink-0 font-medium font-mono text-xs">{name}</span>
    <ToolStatusBadge state={state} />
    {children}
  </CollapsibleTrigger>
);

export type ToolContentProps = ComponentProps<typeof CollapsibleContent>;

export const ToolContent = ({ className, ...props }: ToolContentProps) => (
  <CollapsibleContent
    className={cn(
      "text-popover-foreground outline-none",
      "data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in",
      className
    )}
    {...props}
  />
);

/** Section heading shared by the parameters and result panes. */
const ToolSectionLabel = ({
  children,
  isError = false,
}: {
  readonly children: ReactNode;
  readonly isError?: boolean;
}) => (
  <h4
    className={cn(
      "px-3 pt-2 font-medium text-xs uppercase tracking-wide",
      isError ? "text-destructive" : "text-muted-foreground"
    )}
  >
    {children}
  </h4>
);

/** A copyable, highlighted payload pane. */
const ToolPayload = ({
  code,
  language,
}: {
  readonly code: string;
  readonly language: CodeBlockLanguage;
}) => (
  <CodeBlock
    className="[&_pre]:whitespace-pre-wrap! [&_pre]:wrap-break-word! rounded-none border-0 border-t [&_code]:text-[11px]! [&_pre]:p-3! [&_pre]:text-[11px]! [&_pre]:leading-relaxed!"
    code={code}
    language={language}
  >
    <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
  </CodeBlock>
);

export type ToolInputProps = ComponentProps<"div"> & {
  readonly code: string;
  readonly language: CodeBlockLanguage;
  /** Overrides the "Parameters" heading, e.g. with the unwrapped arg's name. */
  readonly label?: string;
};

export const ToolInput = ({
  className,
  code,
  label = "Parameters",
  language,
  ...props
}: ToolInputProps) => (
  <div className={cn("overflow-hidden", className)} {...props}>
    <ToolSectionLabel>{label}</ToolSectionLabel>
    <ToolPayload code={code} language={language} />
  </div>
);

const ROW_STYLES: Record<DiffRowKind, string> = {
  add: "bg-emerald-500/10",
  ctx: "",
  del: "bg-red-500/10",
  gap: "",
};

const ROW_SIGNS: Record<DiffRowKind, string> = {
  add: "+",
  ctx: " ",
  del: "-",
  gap: "",
};

/** Gutter cell: the line's number on one side, blank on the side it is absent. */
const DiffNo = ({ no }: { readonly no: number | undefined }) => (
  <span className="w-8 shrink-0 select-none pr-2 text-right text-muted-foreground/50">
    {no ?? ""}
  </span>
);

const DiffLineRow = ({
  line,
  row,
}: {
  readonly line: CodeLine | undefined;
  readonly row: DiffRow;
}) => (
  <div className={cn("flex items-start", ROW_STYLES[row.kind])}>
    <DiffNo no={row.oldNo} />
    <DiffNo no={row.newNo} />
    <span
      className={cn(
        "w-4 shrink-0 select-none text-center",
        row.kind === "add" && "text-emerald-500",
        row.kind === "del" && "text-red-400"
      )}
    >
      {ROW_SIGNS[row.kind]}
    </span>
    <span className="wrap-break-word min-w-0 flex-1 whitespace-pre-wrap pr-3">
      <CodeTokens line={line} />
    </span>
  </div>
);

export type ToolDiffProps = ComponentProps<"div"> & {
  readonly diff: LineDiff;
  readonly label?: string;
  readonly language: CodeBlockLanguage;
  readonly newCode: string;
  readonly oldCode: string;
};

/**
 * An edit rendered the way a review tool renders one: both sides of every
 * changed line in a single block, unchanged lines for context, longer
 * unchanged stretches collapsed to a count. Each side is highlighted whole, so
 * a token that spans lines is coloured by the file it came from rather than by
 * the fragment a row happens to hold.
 */
export const ToolDiff = ({
  className,
  diff,
  label = "Diff",
  language,
  newCode,
  oldCode,
  ...props
}: ToolDiffProps) => {
  const before = useMemo(
    () => highlightCode(oldCode, language),
    [oldCode, language]
  );
  const after = useMemo(
    () => highlightCode(newCode, language),
    [newCode, language]
  );
  return (
    <div className={cn("overflow-hidden", className)} {...props}>
      <ToolSectionLabel>
        {label}
        <span className="ml-2 font-normal text-emerald-500">
          +{diff.added}
        </span>
        <span className="ml-1 font-normal text-red-400">−{diff.removed}</span>
      </ToolSectionLabel>
      <div
        className="overflow-auto border-t py-1 font-mono text-[11px] leading-relaxed"
        style={CODE_SURFACE}
      >
        {diff.rows.map((row, i) =>
          row.kind === "gap" ? (
            <div
              className="select-none px-3 py-1 text-muted-foreground/60"
              key={`gap-${i}-${row.count}`}
            >
              ⋯ {row.count} unchanged {row.count === 1 ? "line" : "lines"}
            </div>
          ) : (
            <DiffLineRow
              key={`${row.kind}-${row.oldNo ?? ""}-${row.newNo ?? ""}`}
              line={
                row.kind === "del"
                  ? before[(row.oldNo ?? 1) - 1]
                  : after[(row.newNo ?? 1) - 1]
              }
              row={row}
            />
          )
        )}
      </div>
    </div>
  );
};

export type ToolOutputProps = ComponentProps<"div"> & {
  readonly code: string;
  readonly isError?: boolean;
  readonly language: CodeBlockLanguage;
  readonly label?: string;
};

export const ToolOutput = ({
  className,
  code,
  isError = false,
  label,
  language,
  ...props
}: ToolOutputProps) => (
  <div
    className={cn(
      "overflow-hidden",
      isError && "bg-destructive/10",
      className
    )}
    {...props}
  >
    <ToolSectionLabel isError={isError}>
      {label ?? (isError ? "Error" : "Result")}
    </ToolSectionLabel>
    <ToolPayload code={code} language={language} />
  </div>
);
